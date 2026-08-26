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
        let href = format!("/{}", encode_path(&path));
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
    Html(page(
        "Recently opened",
        &format!("<main><h1>Recently opened</h1><ul class=\"recent\">{list}</ul></main>"),
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
    let mut body = String::new();
    let parser = Parser::new_ext(&source, Options::all());
    html::push_html(&mut body, parser);
    let title = path.file_name().unwrap_or_default().to_string_lossy();
    Html(page(&title, &format!("<article>{body}</article>"))).into_response()
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

fn file_url(port: u16, path: &Path) -> String {
    format!(
        "http://localhost:{}/{}",
        port,
        encode_path(path.to_string_lossy().trim_start_matches('/'))
    )
}

fn page(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{}</title><style>
:root {{ color-scheme: light dark; --bg:#fff; --fg:#24292f; --muted:#656d76; --line:#d0d7de; --code:#f6f8fa; --link:#0969da }}
@media(prefers-color-scheme:dark) {{ :root{{--bg:#0d1117;--fg:#e6edf3;--muted:#8b949e;--line:#30363d;--code:#161b22;--link:#58a6ff}} }}
*{{box-sizing:border-box}} body{{margin:0;background:var(--bg);color:var(--fg);font:16px/1.6 system-ui,-apple-system,sans-serif}} article,main{{max-width:900px;margin:0 auto;padding:48px 28px 96px}} h1,h2{{border-bottom:1px solid var(--line);padding-bottom:.3em}} a{{color:var(--link)}} pre{{overflow:auto;padding:16px;background:var(--code);border-radius:8px}} code{{background:var(--code);padding:.15em .35em;border-radius:4px}} pre code{{padding:0}} blockquote{{color:var(--muted);border-left:4px solid var(--line);margin-left:0;padding-left:1em}} img{{max-width:100%}} table{{border-collapse:collapse}} th,td{{border:1px solid var(--line);padding:6px 13px}} .recent{{list-style:none;padding:0}} .recent li{{border-bottom:1px solid var(--line)}} .recent a{{display:flex;flex-direction:column;padding:14px 4px;text-decoration:none}} .recent small{{color:var(--muted);overflow-wrap:anywhere}} .empty{{color:var(--muted);padding:20px 4px}}
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
}
