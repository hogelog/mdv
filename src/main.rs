use axum::{
    extract::{Path as AxumPath, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use chrono::{DateTime, Utc};
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
use usage::Cli;

const DAEMON_ENV: &str = "MDV_DAEMON";

/// View local Markdown files in your browser.
#[derive(Cli, Debug)]
#[usage(bin = "mdv", version)]
struct Args {
    /// Start the background server.
    #[usage(long)]
    start: bool,

    /// Stop the background server.
    #[usage(long)]
    stop: bool,

    /// Port for the local server.
    #[usage(long, short = 'p', default = "8088")]
    port: u16,

    /// Markdown file to open.
    file: Option<PathBuf>,
}

#[derive(Clone)]
struct AppState {
    config_dir: PathBuf,
    port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HistoryEntry {
    path: PathBuf,
    opened_at: DateTime<Utc>,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("mdv: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
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
        .route("/{*path}", get(markdown))
        .with_state(state);
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
<header class="home-header"><a class="brand" href="/"><span class="brand-mark">M↓</span><span><strong>mdv</strong><small>Local Markdown Viewer</small></span></a><span class="status"><i></i> daemon :{}</span></header>
<main class="home-main"><section class="hero"><p>Pass a local file to <code>mdv</code>.</p><pre><code>mdv path/to/file.md</code></pre></section>
<section class="recent-section"><div class="section-heading"><div><p class="eyebrow">Library</p><h2>Recently opened</h2></div><span>Up to 30 files</span></div><ul class="recent">{list}</ul></section></main>
<footer class="home-footer"><span>mdv 0.1.0</span><span>Data: {}</span><span>Bound to 127.0.0.1</span></footer></div>"#,
            state.port,
            html_escape::encode_text(&config_path),
        ),
    ))
}

async fn markdown(AxumPath(raw): AxumPath<String>, State(state): State<AppState>) -> Response {
    let decoded = match percent_decode_str(&raw).decode_utf8() {
        Ok(value) => value,
        Err(_) => return error_page(StatusCode::BAD_REQUEST, "Invalid path"),
    };
    let path = PathBuf::from(format!("/{decoded}"));
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
    Html(page(
        &title,
        &format!(
            "<div class=\"wrap\"><nav class=\"toc\"><div class=\"toctitle\">Contents</div>{toc}</nav><main>{body}</main></div>"
        ),
    ))
    .into_response()
}

fn render_markdown(source: &str) -> (String, String) {
    let mut rendered = String::new();
    html::push_html(&mut rendered, Parser::new_ext(source, Options::all()));

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
            opened_at: Utc::now(),
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
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{}</title><style>
:root {{ color-scheme:light dark; --bg:#fff; --fg:#1f2328; --muted:#656d76; --border:#d1d9e0; --accent:#0969da; --code-bg:#f6f8fa; --quote-bg:#f6f8fa; --th-bg:#f6f8fa; --pre-bg:#1f2328; --pre-fg:#e6edf3; --strong:#0a3069 }}
@media(prefers-color-scheme:dark) {{ :root{{--bg:#0d1117;--fg:#e6edf3;--muted:#9198a1;--border:#30363d;--accent:#4493f8;--code-bg:#161b22;--quote-bg:#161b22;--th-bg:#161b22;--pre-bg:#010409;--pre-fg:#e6edf3;--strong:#a5d6ff}} }}
*{{box-sizing:border-box}} html{{scroll-behavior:smooth}} body{{margin:0;background:var(--bg);color:var(--fg);font-family:-apple-system,BlinkMacSystemFont,"Hiragino Sans","Noto Sans JP","Segoe UI",sans-serif;font-size:15px;line-height:1.75}}
.wrap{{display:flex;max-width:1280px;margin:0 auto;gap:32px}} nav.toc{{flex:0 0 260px;position:sticky;top:0;align-self:flex-start;max-height:100vh;overflow-y:auto;padding:32px 0 32px 20px;font-size:13px;line-height:1.5}} nav.toc>.toctitle{{font-weight:600;color:var(--muted);text-transform:uppercase;letter-spacing:.06em;font-size:11px;margin-bottom:10px}} nav.toc ul{{list-style:none;margin:0;padding:0}} nav.toc li{{margin:3px 0}} nav.toc .toc-level-3{{padding-left:12px}} nav.toc a{{color:var(--muted);text-decoration:none;display:block;padding:2px 6px;border-left:2px solid transparent;border-radius:0 4px 4px 0}} nav.toc a:hover{{color:var(--accent);border-left-color:var(--accent);background:var(--code-bg)}} .toc-empty{{color:var(--muted)}}
main{{flex:1 1 auto;min-width:0;max-width:900px;margin:0 auto;padding:32px 24px 96px}} h1{{font-size:28px;line-height:1.4;margin:0 0 8px;padding-bottom:12px;border-bottom:1px solid var(--border);letter-spacing:-.01em}} h2{{font-size:21px;margin:44px 0 12px;padding-bottom:8px;border-bottom:1px solid var(--border);letter-spacing:-.01em}} h3{{font-size:17px;margin:28px 0 10px}} p{{margin:12px 0}} a{{color:var(--accent)}} strong{{color:var(--strong);font-weight:600}} code{{background:var(--code-bg);padding:.15em .4em;border-radius:5px;font-family:ui-monospace,SFMono-Regular,"SF Mono",Menlo,monospace;font-size:.875em;word-break:break-word}} pre{{background:var(--pre-bg);color:var(--pre-fg);padding:16px 18px;border-radius:8px;overflow-x:auto;line-height:1.5;font-size:12.5px}} pre code{{background:none;padding:0;color:inherit;font-size:inherit;white-space:pre}} blockquote{{margin:16px 0;padding:12px 16px;background:var(--quote-bg);border-left:3px solid var(--accent);border-radius:0 6px 6px 0;color:var(--fg)}} blockquote p{{margin:4px 0}} table{{border-collapse:collapse;width:100%;margin:16px 0;font-size:13.5px;display:block;overflow-x:auto}} th,td{{border:1px solid var(--border);padding:8px 12px;text-align:left;vertical-align:top}} th{{background:var(--th-bg);font-weight:600;white-space:nowrap}} tbody tr:nth-child(even){{background:color-mix(in srgb,var(--code-bg) 55%,transparent)}} ul,ol{{padding-left:24px;margin:12px 0}} li{{margin:5px 0}} input[type=checkbox]{{margin-right:8px;accent-color:var(--accent)}} hr{{border:0;border-top:1px solid var(--border);margin:32px 0}} img{{max-width:100%}}
.recent{{list-style:none;padding:0}} .recent li{{border-bottom:1px solid var(--border)}} .recent a{{display:flex;flex-direction:column;padding:14px 4px;text-decoration:none}} .recent small{{color:var(--muted);overflow-wrap:anywhere}} .empty{{color:var(--muted);padding:20px 4px}}
.home-shell{{max-width:1040px;margin:0 auto;padding:0 28px}} .home-header{{height:76px;display:flex;align-items:center;justify-content:space-between;border-bottom:1px solid var(--border)}} .brand{{display:flex;align-items:center;gap:12px;color:var(--fg);text-decoration:none}} .brand-mark{{display:grid;place-items:center;width:38px;height:38px;border-radius:9px;background:var(--fg);color:var(--bg);font:700 15px ui-monospace,SFMono-Regular,monospace}} .brand strong{{display:block;color:var(--fg);font-size:18px;line-height:1.15;letter-spacing:-.02em}} .brand small{{display:block;color:var(--muted);font-size:11px;line-height:1.4}} .status{{display:flex;align-items:center;gap:7px;color:var(--muted);font:12px ui-monospace,SFMono-Regular,monospace}} .status i{{width:7px;height:7px;border-radius:50%;background:#1a7f37;box-shadow:0 0 0 3px color-mix(in srgb,#1a7f37 15%,transparent)}} .home-main{{max-width:none;padding:64px 0 80px}} .hero{{max-width:680px}} .eyebrow{{margin:0 0 8px;color:var(--accent);font-size:11px;font-weight:700;letter-spacing:.1em;text-transform:uppercase}} .hero h1{{font-size:38px;border:0;padding:0;margin:0 0 16px;letter-spacing:-.035em}} .hero>p:not(.eyebrow){{max-width:620px;color:var(--muted);font-size:16px}} .hero pre{{display:inline-block;margin:18px 0 0;padding:12px 16px}} .recent-section{{margin-top:72px}} .section-heading{{display:flex;align-items:flex-end;justify-content:space-between;border-bottom:1px solid var(--border);padding-bottom:10px}} .section-heading h2{{border:0;margin:0;padding:0;font-size:21px}} .section-heading>span{{color:var(--muted);font-size:12px}} .recent{{margin:0}} .recent a{{padding:16px 4px}} .recent strong{{color:var(--fg)}} .recent a:hover strong{{color:var(--accent)}} .home-footer{{display:flex;gap:24px;flex-wrap:wrap;padding:18px 0 32px;border-top:1px solid var(--border);color:var(--muted);font:11px ui-monospace,SFMono-Regular,monospace}}
@media(max-width:900px){{.wrap{{display:block}}nav.toc{{position:static;max-height:none;padding:24px 24px 18px;flex:none;border-bottom:1px solid var(--border)}}main{{padding:24px 20px 64px}}}}
@media(max-width:600px){{.home-shell{{padding:0 20px}}.home-header{{height:68px}}.brand small{{display:none}}.status{{font-size:11px}}.home-main{{padding:44px 0 64px}}.hero h1{{font-size:31px}}.recent-section{{margin-top:52px}}.home-footer{{gap:10px;flex-direction:column}}}}
</style></head><body>{body}</body></html>"#,
        html_escape::encode_text(title)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(toc.contains("class=\"toc-level-3\""));
    }
}
