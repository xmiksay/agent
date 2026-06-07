//! SPA assets baked into the binary at compile time. `frontend/dist/` must
//! exist when `cargo build` runs — produce it with `cd frontend && npm run
//! build`. Unknown paths fall through to `index.html` so the Vue router can
//! handle them client-side.

use axum::body::Body;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "frontend/dist/"]
struct Assets;

pub async fn handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if let Some(file) = Assets::get(path) {
        return respond(path, file.data.into_owned());
    }
    match Assets::get("index.html") {
        Some(index) => respond("index.html", index.data.into_owned()),
        None => (StatusCode::NOT_FOUND, "SPA not built").into_response(),
    }
}

fn respond(path: &str, bytes: Vec<u8>) -> Response {
    let ct = match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("webmanifest") => "application/manifest+json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("map") => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, ct)
        .body(Body::from(bytes))
        .unwrap()
}
