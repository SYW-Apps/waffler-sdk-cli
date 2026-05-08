use anyhow::{bail, Context, Result};
use clap::Args;
use console::style;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

// ─── CLI args ─────────────────────────────────────────────────────────────────

#[derive(Args)]
pub struct DevArgs {
    /// UI plugin name; resolves to <cwd>/.ui/<name>/ if cwd is a package root
    #[arg(short = 't', long = "target")]
    pub target: Option<String>,

    /// Explicit absolute or relative path to a UI plugin folder (overrides --target)
    #[arg(short = 'p', long = "path")]
    pub path: Option<PathBuf>,

    /// Port to serve on (default: 9876)
    #[arg(long, default_value = "9876")]
    pub port: u16,

    /// Disable .dev-mocks.json proxy
    #[arg(long)]
    pub no_mocks: bool,
}

// ─── Plugin descriptor extracted from package.json / plugin.json ──────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PluginDescriptor {
    id: String,
    display_name: String,
    version: String,
}

// ─── Shared server state ──────────────────────────────────────────────────────

struct DevServerState {
    /// Monotonically increasing version string (timestamp-based)
    current_version: u64,
    /// Reload broadcast channel sender
    reload_tx: broadcast::Sender<String>,
}

impl DevServerState {
    fn new(reload_tx: broadcast::Sender<String>) -> Self {
        Self {
            current_version: timestamp_version(),
            reload_tx,
        }
    }

    fn bump_version(&mut self) -> u64 {
        self.current_version = timestamp_version();
        let _ = self.reload_tx.send(self.current_version.to_string());
        self.current_version
    }
}

fn timestamp_version() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ─── Plugin path resolution ───────────────────────────────────────────────────

fn resolve_plugin_path(args: &DevArgs) -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;

    // Explicit path wins
    if let Some(ref p) = args.path {
        let abs = if p.is_absolute() { p.clone() } else { cwd.join(p) };
        return Ok(abs.canonicalize().with_context(|| {
            format!("Path does not exist: {:?}", abs)
        })?);
    }

    // Named target: look in .ui/<name>/
    if let Some(ref name) = args.target {
        let candidate = cwd.join(".ui").join(name);
        if candidate.exists() {
            return Ok(candidate
                .canonicalize()
                .context("Failed to canonicalize .ui/<target> path")?);
        }
        bail!(
            "UI plugin '{}' not found at {:?}\nHint: run from your package root where .ui/{} exists",
            name,
            cwd.join(".ui").join(name),
            name
        );
    }

    // Auto-detect: if cwd has vite.config.ts + src/index.tsx, use it
    if cwd.join("vite.config.ts").exists() && cwd.join("src").join("index.tsx").exists() {
        return Ok(cwd);
    }

    // Last resort: list .ui/* candidates and bail with usage message
    let ui_dir = cwd.join(".ui");
    if ui_dir.exists() {
        let candidates: Vec<_> = std::fs::read_dir(&ui_dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();

        if !candidates.is_empty() {
            bail!(
                "No UI plugin specified and current directory is not a plugin root.\n\
                 Available plugins in {}/.ui/:\n{}\n\n\
                 Usage: waffler ui dev --target <name>",
                cwd.display(),
                candidates.iter().map(|c| format!("  - {c}")).collect::<Vec<_>>().join("\n")
            );
        }
    }

    bail!(
        "No UI plugin found. Run from a plugin directory (with vite.config.ts + src/index.tsx)\n\
         or from a package root with a .ui/ directory.\n\n\
         Usage: waffler ui dev [--target <name>] [--path <path>]"
    );
}

// ─── Descriptor extraction ────────────────────────────────────────────────────

/// Build descriptor from the plugin's `definePlugin({ manifest: {...} })` call
/// in src/index.tsx — this is the SAME id the installed plugin uses at runtime,
/// so the override-by-id flow in waffler_ui matches the registry entry.
///
/// Fallbacks:
///   1. src/index.tsx definePlugin manifest (preferred — runtime truth)
///   2. plugin.json (explicit override)
///   3. package.json `waffler` field
///   4. package.json `name` (last resort — only matches if dev author named the
///      package the same as the runtime plugin id, which is uncommon)
fn load_descriptor(plugin_path: &std::path::Path) -> Result<PluginDescriptor> {
    // 1. Prefer manifest extracted from src/index.tsx — this matches the id the
    //    plugin self-registers under at runtime, so dev plugins override
    //    installed plugins of the same id rather than appearing as a duplicate.
    let index_path = plugin_path.join("src").join("index.tsx");
    if index_path.exists() {
        let source = std::fs::read_to_string(&index_path).unwrap_or_default();
        // Common idiom: `const PLUGIN_ID = 'syw.foo.ui'` then `id: PLUGIN_ID` in
        // the manifest. Resolve the constant reference.
        let id = extract_const_string(&source, "PLUGIN_ID")
            .or_else(|| extract_manifest_field(&source, "id"));
        if let Some(id) = id {
            let display_name = extract_manifest_field(&source, "displayName")
                .unwrap_or_else(|| id.clone());
            let version =
                extract_manifest_field(&source, "version").unwrap_or_else(|| "0.1.0".into());
            return Ok(PluginDescriptor { id, display_name, version });
        }
    }

    // 2. Explicit plugin.json
    let plugin_json_path = plugin_path.join("plugin.json");
    if plugin_json_path.exists() {
        let raw = std::fs::read_to_string(&plugin_json_path)
            .context("Failed to read plugin.json")?;
        let v: serde_json::Value =
            serde_json::from_str(&raw).context("Failed to parse plugin.json")?;

        let id = v["id"].as_str().unwrap_or("unknown").to_string();
        let display_name = v["display_name"]
            .as_str()
            .unwrap_or(&id)
            .to_string();
        let version = v["version"].as_str().unwrap_or("0.1.0").to_string();

        return Ok(PluginDescriptor { id, display_name, version });
    }

    // 3 + 4. package.json
    let pkg_json_path = plugin_path.join("package.json");
    if pkg_json_path.exists() {
        let raw = std::fs::read_to_string(&pkg_json_path)
            .context("Failed to read package.json")?;
        let v: serde_json::Value =
            serde_json::from_str(&raw).context("Failed to parse package.json")?;

        if let Some(waffler) = v.get("waffler") {
            let id = waffler["id"]
                .as_str()
                .or_else(|| v["name"].as_str())
                .unwrap_or("unknown")
                .to_string();
            let display_name = waffler["displayName"]
                .as_str()
                .unwrap_or(&id)
                .to_string();
            let version = v["version"].as_str().unwrap_or("0.1.0").to_string();
            return Ok(PluginDescriptor { id, display_name, version });
        }

        let name = v["name"].as_str().unwrap_or("unknown").to_string();
        let version = v["version"].as_str().unwrap_or("0.1.0").to_string();
        return Ok(PluginDescriptor {
            id: name.clone(),
            display_name: name,
            version,
        });
    }

    bail!("No src/index.tsx, plugin.json, or package.json found at {:?}", plugin_path);
}

/// Best-effort regex extraction of a string field from a definePlugin manifest literal.
fn extract_manifest_field(source: &str, field: &str) -> Option<String> {
    let pattern = format!(r#"["']?{field}["']?\s*:\s*["']([^"']+)["']"#);
    let re = regex::Regex::new(&pattern).ok()?;
    re.captures(source)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

/// Find `const|let|var <ident> = '...'` in source; returns the literal string.
fn extract_const_string(source: &str, ident: &str) -> Option<String> {
    let pattern = format!(r#"(?:const|let|var)\s+{ident}\s*(?::\s*\w+)?\s*=\s*["']([^"']+)["']"#);
    let re = regex::Regex::new(&pattern).ok()?;
    re.captures(source)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

// ─── Load dev mocks ───────────────────────────────────────────────────────────

fn load_dev_mocks(plugin_path: &std::path::Path) -> HashMap<String, serde_json::Value> {
    let mocks_path = plugin_path.join(".dev-mocks.json");
    if !mocks_path.exists() {
        return HashMap::new();
    }
    std::fs::read_to_string(&mocks_path)
        .ok()
        .and_then(|s| serde_json::from_str::<HashMap<String, serde_json::Value>>(&s).ok())
        .unwrap_or_default()
}

// ─── Axum HTTP server ─────────────────────────────────────────────────────────

use axum::{
    extract::{Path, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response, Sse},
    routing::get,
    Router,
};
use axum::response::sse::Event;
use tokio_stream::wrappers::BroadcastStream;
use futures::stream::StreamExt;

#[derive(Clone)]
struct AppState {
    plugin_path: PathBuf,
    descriptor: PluginDescriptor,
    port: u16,
    dev_mocks: HashMap<String, serde_json::Value>,
    no_mocks: bool,
    state: Arc<Mutex<DevServerState>>,
}

async fn handle_plugin_mjs(State(app): State<AppState>) -> Response {
    let mjs_path = app.plugin_path.join("dist").join("plugin.mjs");
    match tokio::fs::read(&mjs_path).await {
        Ok(bytes) => {
            let mut res = bytes.into_response();
            res.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/javascript"),
            );
            res.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("no-cache"),
            );
            res.headers_mut().insert(
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            );
            res
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            "plugin.mjs not found — run `npm run build` (or `vite build --watch`) first",
        )
            .into_response(),
    }
}

async fn handle_plugin_json(State(app): State<AppState>) -> Response {
    let version = {
        let state = app.state.lock().await;
        state.current_version.to_string()
    };
    let json = serde_json::json!({
        "id": app.descriptor.id,
        "displayName": app.descriptor.display_name,
        "version": version,
        "asset_url": format!("http://localhost:{}/plugin.mjs", app.port),
    });
    let body = serde_json::to_string_pretty(&json).unwrap_or_default();
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response()
}

async fn handle_events(State(app): State<AppState>) -> Response {
    let rx = app.state.lock().await.reload_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|msg| async move {
        match msg {
            Ok(version) => {
                let data = serde_json::json!({ "version": version }).to_string();
                Some(Ok::<Event, axum::BoxError>(Event::default().event("reload").data(data)))
            }
            Err(_) => None,
        }
    });

    let sse = Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("ping"),
    );

    let mut response = sse.into_response();
    // Browser EventSource is a cross-origin request — needs CORS even for SSE.
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    response
}

async fn handle_sdk_mock(
    State(app): State<AppState>,
    Path(topic): Path<String>,
) -> Response {
    if app.no_mocks {
        return (StatusCode::NOT_FOUND, "Mocks disabled (--no-mocks)").into_response();
    }

    match app.dev_mocks.get(&topic) {
        Some(payload) => {
            let body = serde_json::to_string(payload).unwrap_or_default();
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "application/json"),
                    (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
                ],
                body,
            )
                .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            format!("No mock declared for topic '{topic}'"),
        )
            .into_response(),
    }
}

async fn handle_cors_preflight() -> Response {
    (
        StatusCode::OK,
        [
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*"),
            (header::ACCESS_CONTROL_ALLOW_METHODS, "GET, OPTIONS"),
            (header::ACCESS_CONTROL_ALLOW_HEADERS, "Content-Type"),
        ],
    )
        .into_response()
}

// ─── File watcher ─────────────────────────────────────────────────────────────

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

fn spawn_file_watcher(
    plugin_path: PathBuf,
    state: Arc<Mutex<DevServerState>>,
) -> Result<RecommendedWatcher> {
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            use notify::EventKind::*;
            if matches!(event.kind, Modify(_) | Create(_)) {
                let _ = tx.blocking_send(());
            }
        }
    })?;

    let watch_path = plugin_path.join("dist");
    if !watch_path.exists() {
        std::fs::create_dir_all(&watch_path).ok();
    }
    watcher.watch(&watch_path, RecursiveMode::NonRecursive)?;

    tokio::spawn(async move {
        let mut debounce = tokio::time::interval(std::time::Duration::from_millis(200));
        let mut pending = false;

        loop {
            tokio::select! {
                Some(()) = rx.recv() => {
                    pending = true;
                    // Reset debounce timer by consuming the next tick immediately
                    debounce.reset();
                }
                _ = debounce.tick() => {
                    if pending {
                        pending = false;
                        let new_version = state.lock().await.bump_version();
                        println!(
                            "  {} plugin.mjs changed — reload event sent (v{})",
                            style("↻").cyan().bold(),
                            new_version
                        );
                    }
                }
            }
        }
    });

    Ok(watcher)
}

// ─── Vite child process ───────────────────────────────────────────────────────

async fn spawn_vite_watch(plugin_path: &std::path::Path) -> Result<tokio::process::Child> {
    let is_windows = cfg!(windows);
    let vite_bin = if is_windows { "vite.cmd" } else { "vite" };
    let npx_bin = if is_windows { "npx.cmd" } else { "npx" };
    let npm_bin = if is_windows { "npm.cmd" } else { "npm" };

    // 1. Try local node_modules/.bin/vite (works without npx; fastest)
    let local_vite = plugin_path.join("node_modules").join(".bin").join(vite_bin);
    if local_vite.exists() {
        let child = tokio::process::Command::new(&local_vite)
            .args(["build", "--watch"])
            .current_dir(plugin_path)
            .spawn()
            .with_context(|| format!("Failed to spawn local vite at {:?}", local_vite))?;
        return Ok(child);
    }

    // 2. Fall back to npx (Windows: npx.cmd)
    if let Ok(child) = tokio::process::Command::new(npx_bin)
        .args(["vite", "build", "--watch"])
        .current_dir(plugin_path)
        .spawn()
    {
        return Ok(child);
    }

    // 3. Fall back to npm run watch (Windows: npm.cmd)
    let child = tokio::process::Command::new(npm_bin)
        .args(["run", "watch"])
        .current_dir(plugin_path)
        .spawn()
        .context(
            "Failed to spawn vite via local node_modules/.bin/, npx, and npm.\n\
             Hint: run `npm install` inside the plugin directory first to populate node_modules/.bin/vite.",
        )?;
    Ok(child)
}

// ─── Entry point ──────────────────────────────────────────────────────────────

pub async fn run(args: DevArgs) -> Result<()> {
    let plugin_path = resolve_plugin_path(&args)?;
    let descriptor = load_descriptor(&plugin_path)?;
    let dev_mocks = if args.no_mocks {
        HashMap::new()
    } else {
        load_dev_mocks(&plugin_path)
    };

    let port = args.port;

    // ── Print startup banner ───────────────────────────────────────────────────
    println!();
    println!(
        "{} {}",
        style("Waffler UI Plugin Dev Server").cyan().bold(),
        style("─".repeat(30)).dim()
    );
    println!(
        "  {}  {} v{}",
        style("Plugin:").dim(),
        style(&descriptor.display_name).bold(),
        style(&descriptor.version).yellow()
    );
    println!(
        "  {}    {}",
        style("Path:").dim(),
        style(plugin_path.display()).underlined()
    );
    println!(
        "  {}  {}",
        style("Serving:").dim(),
        style(format!("http://localhost:{}", port)).green().bold()
    );
    if !dev_mocks.is_empty() {
        println!(
            "  {}   {} topics loaded from .dev-mocks.json",
            style("Mocks:").dim(),
            style(dev_mocks.len()).yellow()
        );
    }
    println!();
    println!(
        "  {} Open waffler_ui, append {} to URL",
        style("Activate in browser:").dim(),
        style(format!("?dev_plugins=localhost:{}", port)).cyan().bold()
    );
    println!();

    // ── Shared state ───────────────────────────────────────────────────────────
    let (reload_tx, _) = broadcast::channel::<String>(16);
    let dev_state = Arc::new(Mutex::new(DevServerState::new(reload_tx.clone())));

    // ── File watcher ───────────────────────────────────────────────────────────
    let _watcher = spawn_file_watcher(plugin_path.clone(), dev_state.clone())
        .context("Failed to start file watcher on dist/")?;

    // ── Vite child process ─────────────────────────────────────────────────────
    println!(
        "  {} Starting vite build --watch ...",
        style("→").cyan().bold()
    );
    let mut vite_child = spawn_vite_watch(&plugin_path).await?;
    println!(
        "  {} Vite watching for changes (output above)",
        style("✓").green().bold()
    );
    println!();

    // ── Build axum router ──────────────────────────────────────────────────────
    let app_state = AppState {
        plugin_path: plugin_path.clone(),
        descriptor: descriptor.clone(),
        port,
        dev_mocks,
        no_mocks: args.no_mocks,
        state: dev_state,
    };

    let router = Router::new()
        .route("/plugin.mjs", get(handle_plugin_mjs).options(handle_cors_preflight))
        .route("/plugin.json", get(handle_plugin_json).options(handle_cors_preflight))
        .route("/events", get(handle_events))
        .route("/sdk/{topic}", get(handle_sdk_mock).options(handle_cors_preflight))
        .with_state(app_state);

    let addr = format!("127.0.0.1:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to {addr} — is port {port} already in use?"))?;

    println!(
        "  {} Listening on {}",
        style("✓").green().bold(),
        style(format!("http://localhost:{}", port)).green()
    );
    println!("  {} Press Ctrl+C to stop.", style("hint:").dim());
    println!();

    // ── Ctrl+C handler ─────────────────────────────────────────────────────────
    let ctrl_c = tokio::signal::ctrl_c();

    tokio::select! {
        _ = axum::serve(listener, router) => {
            println!("{} Server stopped.", style("→").dim());
        }
        _ = ctrl_c => {
            println!();
            println!("{} Shutting down dev server ...", style("→").cyan().bold());
            // Kill vite child
            let _ = vite_child.kill().await;
            println!("{} Done.", style("✓").green().bold());
        }
    }

    Ok(())
}
