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

use super::{AppState, streaming};

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

/// The canonical directory of the built UI to serve, or `None` to serve no
/// UI.
///
/// An explicit `ui.dir` without an `index.html` is a config error. Without
/// one, a missing `@pnpm/pnpr-ui` package means no UI.
pub(super) fn locate(config: &UiConfig) -> pnpr_error::Result<Option<PathBuf>> {
    if !config.enabled {
        return Ok(None);
    }
    let Some(dir) = &config.dir else {
        return Ok(std::env::current_exe()
            .ok()
            .and_then(|executable| executable.canonicalize().ok())
            .and_then(|executable| ui_dir_beside(&executable))
            .filter(|dir| dir.join(INDEX_FILE).is_file())
            .and_then(|dir| dir.canonicalize().ok()));
    };
    let invalid = |reason: String| RegistryError::InvalidConfig {
        reason: format!("ui.dir {}: {reason}", dir.display()),
    };
    if !dir.join(INDEX_FILE).is_file() {
        return Err(invalid(format!("has no {INDEX_FILE}")));
    }
    dir.canonicalize()
        .map(Some)
        .map_err(|error| invalid(error.to_string()))
}

/// The build in `@pnpm/pnpr-ui`, when `executable` is the `bin/pnpr` of an
/// installed `@pnpm/pnpr` package and the UI package sits beside it. npm and
/// pnpm both place a resolved peer dependency next to the package that
/// declares it. Any other layout gives `None`, so a pnpr binary installed
/// outside npm never serves a directory nobody configured.
pub(super) fn ui_dir_beside(executable: &Path) -> Option<PathBuf> {
    let bin_dir = executable
        .parent()
        .filter(|dir| dir.ends_with("bin"))?;
    let pnpr_package = bin_dir.parent()?;
    let ui_package = pnpr_package.parent()?.join("pnpr-ui");
    (is_package(pnpr_package, "@pnpm/pnpr") && is_package(&ui_package, "@pnpm/pnpr-ui")).then(
        || ui_package.join("dist"),
    )
}

fn is_package(dir: &Path, name: &str) -> bool {
    std::fs::read(dir.join("package.json"))
        .ok()
        .and_then(|manifest| serde_json::from_slice::<serde_json::Value>(&manifest).ok())
        .is_some_and(|manifest| manifest["name"] == name)
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
        Some(file) => contained_file(&dir, &file).await,
        None => None,
    };
    let file = match file {
        Some(file) => file,
        None if relative.starts_with(HASHED_ASSETS_DIR) => {
            return StatusCode::NOT_FOUND.into_response();
        }
        None => return serve_page(&dir, relative).await,
    };
    let opened = match tokio::fs::File::open(&file).await {
        Ok(opened) => opened,
        Err(error) => return read_error(&file, &error),
    };
    let length = match opened.metadata().await {
        Ok(metadata) => metadata.len(),
        Err(error) => return read_error(&file, &error),
    };
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(content_type(&file))),
            (header::CONTENT_LENGTH, HeaderValue::from(length)),
        ],
        streaming::stream_file(opened),
    )
        .into_response()
}

/// `index.html` with a `<base>` pointing back at the UI root, so the
/// bundle's relative asset URLs resolve from any route depth, behind any
/// path prefix.
async fn serve_page(dir: &Path, route: &str) -> Response {
    let Some(file) = contained_file(dir, &dir.join(INDEX_FILE)).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
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

/// The real path of `file` when it is a regular file inside the canonical
/// `dir`. Symlinks are followed, so one that leads out of `dir` gives `None`.
async fn contained_file(dir: &Path, file: &Path) -> Option<PathBuf> {
    let real = tokio::fs::canonicalize(file).await.ok()?;
    let is_file = tokio::fs::metadata(&real).await.is_ok_and(|metadata| metadata.is_file());
    (is_file && real.starts_with(dir)).then_some(real)
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
    let immutable = hashed_asset && !is_html && response.status().is_success();
    let cache_control = if immutable { "public, max-age=31536000, immutable" } else { "no-cache" };
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
