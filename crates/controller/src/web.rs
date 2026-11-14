//! Serves the built web app from `web/dist`, embedded in the binary.
//!
//! The pages hold no data: the app reads everything from `/api`, which only
//! admins may call.

use axum::Json;
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct App;

/// Answers every path that no other route took. A file of the build is
/// served as is; any other path loads `index.html`, so the app's router can
/// handle it. An unknown path under `/api` gets 404 `not_found`.
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path == "api" || path.starts_with("api/") {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "not_found" })),
        )
            .into_response();
    }
    if let Some(response) = file(path) {
        return response;
    }
    if path
        .rsplit('/')
        .next()
        .is_some_and(|name| name.contains('.'))
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    file("index.html").unwrap_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            "The web app is not built. Run `npm run build` in web/ and build sewa-controller again.",
        )
            .into_response()
    })
}

fn file(path: &str) -> Option<Response> {
    let file = App::get(path)?;
    // Vite names the files in assets/ by their content hash, so they never
    // change.
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    let mime = HeaderValue::from_str(file.metadata.mimetype()).ok()?;
    Some(
        (
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
            ],
            file.data,
        )
            .into_response(),
    )
}
