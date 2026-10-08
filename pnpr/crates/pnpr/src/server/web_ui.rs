//! The web UI: the built `@pnpm/pnpr-ui` package, served at [`UI_PATH`] so it
//! calls this server from the same origin.

use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use axum::{
    Router,
    extract::Request,
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse as _, Redirect, Response},
    routing::get,
};
use pnpr_config::UiConfig;
use pnpr_error::RegistryError;

use super::AppState;

const UI_PATH: &str = "/-/ui";
const INDEX_FILE: &str = "index.html";
/// Vite's output directory for content-hashed files.
const HASHED_ASSETS_DIR: &str = "assets/";

/// The UI's own scripts and styles only, plus the theme's Google Fonts and
/// README images from any HTTPS host. `connect-src 'self'` keeps the UI on
/// this server.
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self' \
     'unsafe-inline' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com; \
     img-src 'self' data: https:; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; \
     form-action 'self'";

/// The directory of the built UI to serve, or `None` to serve no UI.
///
/// An explicit `ui.dir` without an `index.html` is a config error. Without
/// one, a missing `@pnpm/pnpr-ui` package means no UI.
pub(super) fn locate(config: &UiConfig) -> pnpr_error::Result<Option<PathBuf>> {
    if !config.enabled {
        return Ok(None);
    }
    match &config.dir {
        Some(dir) if dir.join(INDEX_FILE).is_file() => Ok(Some(dir.clone())),
        Some(dir) => Err(RegistryError::InvalidConfig {
            reason: format!("ui.dir {} has no {INDEX_FILE}", dir.display()),
        }),
        None => Ok(installed_ui_dir().filter(|dir| dir.join(INDEX_FILE).is_file())),
    }
}

/// The build in `@pnpm/pnpr-ui`, looked up beside the `@pnpm/pnpr` package
/// whose `bin/pnpr` this process runs. npm and pnpm both place a resolved
/// peer dependency next to the package that declares it.
fn installed_ui_dir() -> Option<PathBuf> {
    let executable = std::env::current_exe()
        .ok()?
        .canonicalize()
        .ok()?;
    let scope_dir = executable.parent()?.parent()?.parent()?;
    Some(scope_dir.join("pnpr-ui").join("dist"))
}

pub(super) fn routes(dir: &Path) -> Router<AppState> {
    let dir: Arc<Path> = Arc::from(dir);
    let serve = move |request: Request| serve_file(Arc::clone(&dir), request);
    Router::new()
        // A relative redirect, so it also works behind a path prefix.
        .route(UI_PATH, get(|| async { Redirect::to("ui/") }))
        .route(&format!("{UI_PATH}/"), get(serve.clone()))
        .route(&format!("{UI_PATH}/{{*path}}"), get(serve))
        .layer(middleware::from_fn(add_security_headers))
}

/// Serves a file of the UI directory. Any other path outside the hashed
/// assets is one of the UI's own routes and gets the page, so a reload keeps
/// it.
async fn serve_file(dir: Arc<Path>, request: Request) -> Response {
    let relative = request
        .uri()
        .path()
        .strip_prefix(UI_PATH)
        .unwrap_or_default();
    let relative = relative.trim_start_matches('/');
    let file = match file_in(&dir, relative) {
        Some(file) if tokio::fs::metadata(&file).await.is_ok_and(|metadata| metadata.is_file()) => {
            file
        }
        _ if relative.starts_with(HASHED_ASSETS_DIR) => {
            return StatusCode::NOT_FOUND.into_response();
        }
        _ => return serve_page(&dir, relative).await,
    };
    match tokio::fs::read(&file).await {
        Ok(contents) => ([(header::CONTENT_TYPE, content_type(&file))], contents).into_response(),
        Err(error) => read_error(&file, &error),
    }
}

/// `index.html` with a `<base>` pointing back at the UI root, so the
/// bundle's relative asset URLs resolve from any route depth, behind any
/// path prefix.
async fn serve_page(dir: &Path, route: &str) -> Response {
    let file = dir.join(INDEX_FILE);
    let page = match tokio::fs::read_to_string(&file).await {
        Ok(page) => page,
        Err(error) => return read_error(&file, &error),
    };
    let depth = route.matches('/').count();
    let root = if depth == 0 { "./".to_owned() } else { "../".repeat(depth) };
    let page = page.replacen("<head>", &format!(r#"<head><base href="{root}">"#), 1);
    ([(header::CONTENT_TYPE, content_type(&file))], page).into_response()
}

fn read_error(file: &Path, error: &std::io::Error) -> Response {
    tracing::error!(%error, file = %file.display(), "could not read the web UI");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// `relative` inside `dir`, or `None` when it names anything but a plain
/// descendant path.
fn file_in(dir: &Path, relative: &str) -> Option<PathBuf> {
    if relative.contains(['\\', '%', ':']) {
        return None;
    }
    let relative = Path::new(relative);
    relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
        .then(|| dir.join(relative))
}

fn content_type(file: &Path) -> &'static str {
    match file.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

async fn add_security_headers(request: Request, next: Next) -> Response {
    let hashed_asset = request
        .uri()
        .path()
        .strip_prefix(UI_PATH)
        .is_some_and(|path| path.trim_start_matches('/').starts_with(HASHED_ASSETS_DIR));
    let mut response = next.run(request).await;
    let is_html = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/html"));
    let cache_control =
        if hashed_asset && !is_html { "public, max-age=31536000, immutable" } else { "no-cache" };
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_control));
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CONTENT_SECURITY_POLICY),
    );
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    response
}
