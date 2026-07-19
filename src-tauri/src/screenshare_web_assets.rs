use axum::body::Body;
use axum::http::StatusCode;
use axum::response::Response;
use mime_guess::MimeGuess;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../dist/screen-share-web"]
struct ScreenShareWebAssets;

const BUILD_HINT: &str =
    "Screen share web assets are not built. Run `pnpm build:screen-share-web` and rebuild the Tauri app.";

pub fn serve_asset(path: &str) -> Option<Response> {
    let asset_path = normalize_asset_path(path);
    let asset = ScreenShareWebAssets::get(asset_path)?;
    let mime = MimeGuess::from_path(asset_path).first_or_octet_stream();

    Some(
        Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", mime.as_ref())
            .header("Cache-Control", cache_control(asset_path))
            .body(Body::from(asset.data.into_owned()))
            .unwrap(),
    )
}

pub fn serve_index() -> Response {
    let Some(asset) = ScreenShareWebAssets::get("index.html") else {
        return unavailable_response();
    };

    let Ok(html) = String::from_utf8(asset.data.into_owned()) else {
        return unavailable_response();
    };

    if !html.contains("<div id=\"app\"></div>")
        || !html.contains("/assets/")
        || html.contains("/main.ts")
    {
        return unavailable_response();
    }

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Cache-Control", "no-cache")
        .body(Body::from(html))
        .unwrap()
}

pub fn unavailable_response() -> Response {
    Response::builder()
        .status(StatusCode::SERVICE_UNAVAILABLE)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(Body::from(BUILD_HINT))
        .unwrap()
}

fn normalize_asset_path(path: &str) -> &str {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        "index.html"
    } else {
        trimmed
    }
}

fn cache_control(path: &str) -> &'static str {
    if path == "index.html" {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    }
}

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;
    use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};

    use super::*;

    #[test]
    fn embedded_index_is_built_markup() {
        let response = serve_index();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn embedded_index_does_not_reference_dev_sources() {
        let asset =
            ScreenShareWebAssets::get("index.html").expect("screen share index should be embedded");
        let html = String::from_utf8(asset.data.into_owned()).expect("index should be utf-8");
        assert!(html.contains("<div id=\"app\"></div>"));
        assert!(html.contains("rel=\"icon\""));
        assert!(html.contains("/assets/"));
        assert!(!html.contains("/main.ts"));
    }

    #[test]
    fn referenced_assets_use_immutable_cache_headers() {
        let asset_path = ScreenShareWebAssets::iter()
            .find(|path| path.starts_with("assets/") && path.ends_with(".js"))
            .expect("screen share js asset should be embedded");
        let response = serve_asset(&asset_path).expect("js asset should be served");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("public, max-age=31536000, immutable")
        );
        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/javascript")
        );
    }

    #[tokio::test]
    async fn missing_asset_returns_none_and_unavailable_response_is_actionable() {
        assert!(serve_asset("assets/not-present.js").is_none());
        let body = to_bytes(unavailable_response().into_body(), usize::MAX)
            .await
            .expect("response body should be readable");
        assert!(String::from_utf8_lossy(&body).contains("pnpm build:screen-share-web"));
    }

    #[test]
    fn source_index_is_not_accepted_as_build_output() {
        let source = include_str!("../../src/screen-share-web/index.html");
        assert!(!source.contains("/assets/"));
    }
}
