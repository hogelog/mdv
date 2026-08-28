use axum::{
    extract::{Path as AxumPath, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};
use pulldown_cmark::{html, Options, Parser};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs, io,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
const DAEMON_ENV: &str = "MDV_DAEMON";
const HELP: &str = "View local Markdown files in your browser.\n\nUsage: mdv [OPTIONS] [FILE]\n\nOptions:\n      --start        Start the background server\n      --stop         Stop the background server\n  -p, --port PORT    Port for the local server [default: 8088]\n  -h, --help         Print help\n  -V, --version      Print version";

#[derive(Debug, PartialEq)]
struct Args {
    start: bool,
    stop: bool,
    port: u16,
    file: Option<PathBuf>,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut arguments = std::env::args_os();
        arguments.next();
        Self::parse_from(arguments)
    }

    fn parse_from(arguments: impl IntoIterator<Item = std::ffi::OsString>) -> Result<Self, String> {
        let mut args = Self {
            start: false,
            stop: false,
            port: 8088,
            file: None,
        };
        let mut arguments = arguments.into_iter();
        let mut positional_only = false;

        while let Some(argument) = arguments.next() {
            if !positional_only {
                match argument.to_str() {
                    Some("--start") => {
                        args.start = true;
                        continue;
                    }
                    Some("--stop") => {
                        args.stop = true;
                        continue;
                    }
                    Some("-p" | "--port") => {
                        let value = arguments
                            .next()
                            .ok_or_else(|| format!("{argument:?} requires a port"))?;
                        args.port = parse_port(&value)?;
                        continue;
                    }
                    Some("-h" | "--help") => {
                        println!("{HELP}");
                        std::process::exit(0);
                    }
                    Some("-V" | "--version") => {
                        println!("mdv {}", env!("CARGO_PKG_VERSION"));
                        std::process::exit(0);
                    }
                    Some("--") => {
                        positional_only = true;
                        continue;
                    }
                    Some(value) if value.starts_with("--port=") => {
                        args.port = value[7..]
                            .parse()
                            .map_err(|_| format!("invalid port: {}", &value[7..]))?;
                        continue;
                    }
                    Some(value) if value.starts_with('-') => {
                        return Err(format!("unknown option: {value}"));
                    }
                    _ => {}
                }
            }

            if args.file.replace(PathBuf::from(argument)).is_some() {
                return Err("only one Markdown file can be specified".into());
            }
        }
        Ok(args)
    }
}

fn parse_port(value: &std::ffi::OsStr) -> Result<u16, String> {
    value
        .to_str()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| format!("invalid port: {}", value.to_string_lossy()))
}

#[derive(Clone)]
struct AppState {
    config_dir: PathBuf,
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HistoryEntry {
    path: PathBuf,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("mdv: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse().map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    if args.start && args.stop {
        return Err("--start and --stop cannot be used together".into());
    }
    if args.file.is_some() && (args.start || args.stop) {
        return Err("a file cannot be combined with --start or --stop".into());
    }

    let config_dir = config_dir()?;
    fs::create_dir_all(&config_dir)?;

    if std::env::var_os(DAEMON_ENV).is_some() {
        return serve(args.port, config_dir).await;
    }
    if args.stop {
        stop_daemon(&config_dir)?;
        println!("mdv daemon stopped");
        return Ok(());
    }
    if args.start {
        ensure_daemon(args.port, &config_dir)?;
        println!("mdv daemon listening at http://localhost:{}", args.port);
        return Ok(());
    }

    match args.file {
        Some(path) => {
            let path = canonical_markdown(&path)?;
            ensure_daemon(args.port, &config_dir)?;
            add_history(&config_dir, &path)?;
            let url = file_url(args.port, &path);
            open::that(&url)?;
            println!("{url}");
        }
        None => {
            ensure_daemon(args.port, &config_dir)?;
            let url = format!("http://localhost:{}", args.port);
            open::that(&url)?;
            println!("{url}");
        }
    }
    Ok(())
}

fn config_dir() -> io::Result<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|path| path.join(".config")))
        .map(|path| path.join("mdv"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "cannot determine config directory"))
}

fn pid_path(config_dir: &Path) -> PathBuf {
    config_dir.join("daemon.pid")
}

fn port_path(config_dir: &Path) -> PathBuf {
    config_dir.join("daemon.port")
}

fn daemon_alive(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        Duration::from_millis(150),
    )
    .is_ok()
}

fn ensure_daemon(port: u16, config_dir: &Path) -> io::Result<()> {
    if daemon_alive(port) {
        return Ok(());
    }
    if let Ok(other_port) = fs::read_to_string(port_path(config_dir)) {
        if let Ok(other_port) = other_port.trim().parse::<u16>() {
            if daemon_alive(other_port) {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("mdv daemon is already running on port {other_port}"),
                ));
            }
        }
    }

    let executable = std::env::current_exe()?;
    Command::new(executable)
        .arg("--port")
        .arg(port.to_string())
        .env(DAEMON_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(50));
        if daemon_alive(port) {
            return Ok(());
        }
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "daemon did not start",
    ))
}

fn stop_daemon(config_dir: &Path) -> io::Result<()> {
    let pid_file = pid_path(config_dir);
    let pid: i32 = fs::read_to_string(&pid_file)?
        .trim()
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid daemon PID"))?;
    if pid <= 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "refusing invalid daemon PID",
        ));
    }
    let command = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()?;
    let executable = String::from_utf8_lossy(&command.stdout);
    let is_mdv = Path::new(executable.trim())
        .file_name()
        .is_some_and(|name| name == "mdv");
    if !command.status.success() || !is_mdv {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "stale daemon PID file; no mdv process found",
        ));
    }
    let result = unsafe { libc::kill(pid, libc::SIGTERM) };
    if result != 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(error);
        }
    }
    let _ = fs::remove_file(pid_file);
    let _ = fs::remove_file(port_path(config_dir));
    Ok(())
}

async fn serve(port: u16, config_dir: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
    fs::write(pid_path(&config_dir), std::process::id().to_string())?;
    fs::write(port_path(&config_dir), port.to_string())?;

    let state = AppState {
        config_dir: config_dir.clone(),
        port,
    };
    let app = Router::new()
        .route("/", get(index))
        .route("/assets/style.css", get(stylesheet))
        .route("/{*path}", get(markdown))
        .with_state(state)
        .layer(middleware::from_fn(security_headers));
    println!("mdv daemon listening at http://localhost:{port}");
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;
    let _ = fs::remove_file(pid_path(&config_dir));
    let _ = fs::remove_file(port_path(&config_dir));
    result?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut terminate) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {},
                _ = terminate.recv() => {},
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    apply_security_headers(response.headers_mut());
    response
}

fn apply_security_headers(headers: &mut HeaderMap) {
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; style-src 'self'; img-src 'self' data:; script-src 'none'; \
             frame-src 'none'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'; \
             form-action 'none'; connect-src 'none'; font-src 'none'",
        ),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
}

async fn stylesheet() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../assets/style.css"),
    )
}

async fn index(State(state): State<AppState>) -> Html<String> {
    let entries = read_history(&state.config_dir).unwrap_or_default();
    let mut list = String::new();
    for entry in entries {
        if !entry.path.exists() {
            continue;
        }
        let path = entry.path.to_string_lossy();
        let href = path_href(&path);
        list.push_str(&format!(
            "<li><a href=\"{}\"><strong>{}</strong><small>{}</small></a></li>",
            href,
            html_escape::encode_text(
                entry
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .as_ref()
            ),
            html_escape::encode_text(&path),
        ));
    }
    if list.is_empty() {
        list.push_str("<li class=\"empty\">No recently opened files yet.</li>");
    }
    let config_path = state.config_dir.to_string_lossy();
    Html(page(
        "mdv — Local Markdown Viewer",
        &format!(
            r#"<div class="home-shell">
<header class="home-header"><a class="brand" href="/"><span class="brand-mark">M↓</span><span><strong>mdv</strong><small>Local Markdown Viewer</small></span></a></header>
<main class="home-main"><section class="hero"><p>Pass a local file to <code>mdv</code>.</p><pre><code>mdv README.md          # start the daemon if needed and open the file
mdv                    # open the recently-viewed page
mdv --start            # explicitly start the daemon
mdv --stop             # stop it
mdv --start --port 9000</code></pre></section>
<section class="recent-section"><div class="section-heading"><div><p class="eyebrow">Library</p><h2>Recently opened</h2></div><span>Up to 30 files</span></div><ul class="recent">{list}</ul></section></main>
<footer class="home-footer"><span>mdv 0.1.0</span><a href="https://github.com/hogelog/mdv">GitHub</a><span>Data: {}</span><span>Bound to 127.0.0.1:{}</span></footer></div>"#,
            html_escape::encode_text(&config_path),
            state.port,
        ),
    ))
}

async fn markdown(AxumPath(raw): AxumPath<String>, State(state): State<AppState>) -> Response {
    let decoded = match percent_decode_str(&raw).decode_utf8() {
        Ok(value) => value,
        Err(_) => return error_page(StatusCode::BAD_REQUEST, "Invalid path"),
    };
    let path = PathBuf::from(format!("/{decoded}"));
    if let Some(content_type) = image_content_type(&path) {
        let path = match fs::canonicalize(&path) {
            Ok(path) if path.is_file() => path,
            Ok(_) => return error_page(StatusCode::NOT_FOUND, "not a file"),
            Err(error) => return error_page(StatusCode::NOT_FOUND, &error.to_string()),
        };
        return match fs::read(path) {
            Ok(bytes) => (
                [
                    (header::CONTENT_TYPE, content_type),
                    (header::CONTENT_SECURITY_POLICY, "sandbox"),
                    (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                ],
                bytes,
            )
                .into_response(),
            Err(error) => error_page(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
        };
    }
    let path = match canonical_markdown(&path) {
        Ok(path) => path,
        Err(error) => return error_page(StatusCode::NOT_FOUND, &error.to_string()),
    };
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => return error_page(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string()),
    };
    let _ = add_history(&state.config_dir, &path);
    let (body, toc) = render_markdown(&source);
    let title = path.file_name().unwrap_or_default().to_string_lossy();
    let display_path = path.to_string_lossy();
    Html(page(
        &title,
        &format!(
            "<header class=\"document-header\"><a class=\"document-brand\" href=\"/\">mdv</a><span class=\"document-path\" title=\"{}\">{}</span></header><div class=\"wrap\"><nav class=\"toc\"><div class=\"toctitle\">Contents</div>{toc}</nav><main>{body}</main></div>",
            html_escape::encode_double_quoted_attribute(&display_path),
            html_escape::encode_text(&display_path),
        ),
    ))
    .into_response()
}

fn image_content_type(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "apng" => Some("image/apng"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" | "jfif" | "pjpeg" | "pjp" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "svg" => Some("image/svg+xml"),
        "webp" => Some("image/webp"),
        "avif" => Some("image/avif"),
        _ => None,
    }
}

fn render_markdown(source: &str) -> (String, String) {
    use pulldown_cmark::Event;

    let mut rendered = String::new();
    let parser = Parser::new_ext(source, Options::all()).map(|event| match event {
        Event::Html(source) | Event::InlineHtml(source) => Event::Text(source),
        event => event,
    });
    html::push_html(&mut rendered, parser);

    let headings = collect_headings(source);
    let mut body = rendered;
    for (level, _, id) in &headings {
        let opening = format!("<h{level}>");
        let replacement = format!(
            "<h{level} id=\"{}\">",
            html_escape::encode_double_quoted_attribute(id)
        );
        body = body.replacen(&opening, &replacement, 1);
    }
    (body, render_toc(&headings))
}

fn collect_headings(source: &str) -> Vec<(u8, String, String)> {
    use pulldown_cmark::{Event, Tag, TagEnd};

    let mut headings = Vec::new();
    let mut current: Option<(u8, String)> = None;
    let mut ids = HashMap::<String, usize>::new();
    for event in Parser::new_ext(source, Options::all()) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                current = Some((heading_number(level), String::new()));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, text)) = current.take() {
                    let base = heading_id(&text);
                    let count = ids.entry(base.clone()).or_default();
                    let id = if *count == 0 {
                        base
                    } else {
                        format!("{base}-{}", *count)
                    };
                    *count += 1;
                    headings.push((level, text, id));
                }
            }
            Event::Text(text) | Event::Code(text) if current.is_some() => {
                current.as_mut().unwrap().1.push_str(&text);
            }
            _ => {}
        }
    }
    headings
}

fn heading_number(level: pulldown_cmark::HeadingLevel) -> u8 {
    use pulldown_cmark::HeadingLevel;
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn heading_id(text: &str) -> String {
    let mut id = String::new();
    let mut separator = false;
    for character in text.to_lowercase().chars() {
        if character.is_alphanumeric() || matches!(character, '_' | '-') {
            if separator && !id.is_empty() {
                id.push('-');
            }
            separator = false;
            id.push(character);
        } else {
            separator = true;
        }
    }
    if id.is_empty() {
        "section".into()
    } else {
        id
    }
}

fn render_toc(headings: &[(u8, String, String)]) -> String {
    let visible: Vec<_> = headings
        .iter()
        .filter(|(level, _, _)| *level <= 3)
        .collect();
    if visible.is_empty() {
        return "<p class=\"toc-empty\">No headings</p>".into();
    }
    let mut toc = String::from("<ul>");
    for (level, text, id) in visible {
        toc.push_str(&format!(
            "<li class=\"toc-level-{level}\"><a href=\"#{}\">{}</a></li>",
            html_escape::encode_double_quoted_attribute(id),
            html_escape::encode_text(text),
        ));
    }
    toc.push_str("</ul>");
    toc
}

fn error_page(status: StatusCode, message: &str) -> Response {
    let html = Html(page(
        "mdv error",
        &format!(
            "<main><h1>{status}</h1><p>{}</p></main>",
            html_escape::encode_text(message)
        ),
    ));
    (status, [(header::CACHE_CONTROL, "no-store")], html).into_response()
}

fn canonical_markdown(path: &Path) -> io::Result<PathBuf> {
    let path = fs::canonicalize(path)?;
    if !path.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a file"));
    }
    let valid = path
        .extension()
        .and_then(|v| v.to_str())
        .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdown"));
    if !valid {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "only Markdown files can be viewed",
        ));
    }
    Ok(path)
}

fn history_path(config_dir: &Path) -> PathBuf {
    config_dir.join("history.json")
}

fn read_history(config_dir: &Path) -> io::Result<Vec<HistoryEntry>> {
    match fs::read(history_path(config_dir)) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

fn add_history(config_dir: &Path, path: &Path) -> io::Result<()> {
    let mut entries = read_history(config_dir).unwrap_or_default();
    entries.retain(|entry| entry.path != path);
    entries.insert(
        0,
        HistoryEntry {
            path: path.to_owned(),
        },
    );
    entries.truncate(30);
    let temporary = config_dir.join("history.json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(&entries).map_err(io::Error::other)?,
    )?;
    fs::rename(temporary, history_path(config_dir))
}

fn encode_path(path: &str) -> String {
    path.split('/')
        .map(|part| utf8_percent_encode(part, NON_ALPHANUMERIC).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn path_href(path: &str) -> String {
    format!("/{}", encode_path(path.trim_start_matches('/')))
}

fn file_url(port: u16, path: &Path) -> String {
    format!(
        "http://localhost:{}{}",
        port,
        path_href(&path.to_string_lossy())
    )
}

fn page(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{}</title><link rel="icon" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'%3E%3Crect width='64' height='64' rx='14' fill='%231f2328'/%3E%3Ctext x='32' y='41' text-anchor='middle' fill='white' font-family='monospace' font-size='25' font-weight='700'%3EM%E2%86%93%3C/text%3E%3C/svg%3E"><link rel="stylesheet" href="/assets/style.css">
</head><body>{body}</body></html>"#,
        html_escape::encode_text(title)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn parse_args(arguments: &[&str]) -> Result<Args, String> {
        Args::parse_from(arguments.iter().map(OsString::from))
    }

    #[test]
    fn parses_cli_options_and_file() {
        assert_eq!(
            parse_args(&["--start", "--port=9000"]).unwrap(),
            Args {
                start: true,
                stop: false,
                port: 9000,
                file: None,
            }
        );
        assert_eq!(
            parse_args(&["-p", "3000", "README.md"]).unwrap().file,
            Some(PathBuf::from("README.md"))
        );
    }

    #[test]
    fn rejects_invalid_cli_arguments() {
        assert!(parse_args(&["--port", "invalid"]).is_err());
        assert!(parse_args(&["--unknown"]).is_err());
        assert!(parse_args(&["one.md", "two.md"]).is_err());
    }

    #[test]
    fn url_encodes_spaces() {
        assert_eq!(encode_path("Users/me/a b.md"), "Users/me/a%20b%2Emd");
    }

    #[test]
    fn absolute_path_href_has_one_leading_slash() {
        assert_eq!(path_href("/Users/me/a b.md"), "/Users/me/a%20b%2Emd");
    }

    #[test]
    fn history_is_most_recent_first_and_deduplicated() {
        let dir = tempfile::tempdir().unwrap();
        let one = dir.path().join("one.md");
        let two = dir.path().join("two.md");
        fs::write(&one, "one").unwrap();
        fs::write(&two, "two").unwrap();
        add_history(dir.path(), &one).unwrap();
        add_history(dir.path(), &two).unwrap();
        add_history(dir.path(), &one).unwrap();
        let entries = read_history(dir.path()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, one);
    }

    #[test]
    fn markdown_has_heading_ids_and_toc() {
        let (body, toc) = render_markdown("# Design\n\n## 全体の流れ\n\n### Details");
        assert!(body.contains("<h1 id=\"design\">Design</h1>"));
        assert!(body.contains("<h2 id=\"全体の流れ\">全体の流れ</h2>"));
        assert!(toc.contains("href=\"#全体の流れ\""));
        assert!(toc.contains("class=\"toc-level-1\""));
        assert!(toc.contains("class=\"toc-level-2\""));
        assert!(toc.contains("class=\"toc-level-3\""));
    }

    #[test]
    fn markdown_escapes_raw_html() {
        let (body, _) = render_markdown(
            "<script>alert('xss')</script>\n\n<span onclick=\"alert('xss')\">text</span>",
        );
        assert!(!body.contains("<script>"));
        assert!(!body.contains("<span"));
        assert!(body.contains("&lt;script&gt;"));
        assert!(body.contains("&lt;span onclick=\"alert('xss')\"&gt;"));
    }

    #[test]
    fn recognizes_supported_image_types() {
        assert_eq!(
            image_content_type(Path::new("image.PNG")),
            Some("image/png")
        );
        assert_eq!(
            image_content_type(Path::new("photo.jpeg")),
            Some("image/jpeg")
        );
        assert_eq!(
            image_content_type(Path::new("diagram.svg")),
            Some("image/svg+xml")
        );
        assert_eq!(
            image_content_type(Path::new("animation.apng")),
            Some("image/apng")
        );
        assert_eq!(image_content_type(Path::new("document.pdf")), None);
    }

    #[test]
    fn page_includes_data_uri_favicon() {
        let output = page("Test", "<main>Test</main>");
        assert!(output.contains("rel=\"icon\""));
        assert!(output.contains("data:image/svg+xml"));
        assert!(output.contains("href=\"/assets/style.css\""));
        assert!(!output.contains("<style>"));
    }

    #[test]
    fn applies_security_headers() {
        let mut headers = HeaderMap::new();
        apply_security_headers(&mut headers);
        let policy = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
        assert!(policy.contains("default-src 'none'"));
        assert!(policy.contains("style-src 'self'"));
        assert!(policy.contains("script-src 'none'"));
        assert_eq!(headers[header::REFERRER_POLICY], "no-referrer");
        assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    }
}
