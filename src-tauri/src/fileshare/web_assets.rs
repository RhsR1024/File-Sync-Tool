use axum::body::Body;
use axum::http::StatusCode;
use axum::response::Response;
use mime_guess::MimeGuess;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../dist/file-share-web"]
struct FileShareWebAssets;

pub fn serve_asset(path: &str) -> Option<Response> {
    let asset_path = normalize_asset_path(path);
    let asset = FileShareWebAssets::get(asset_path)?;
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
    serve_asset("index.html").unwrap_or_else(|| {
        Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(Body::from(
                "File share web assets are not built. Run `pnpm build:file-share-web` first.",
            ))
            .unwrap()
    })
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
    use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};

    use super::*;

    #[test]
    fn serves_embedded_file_share_index() {
        let response = serve_index();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn serves_actual_built_index_markup() {
        let asset = FileShareWebAssets::get("index.html")
            .expect("built file share index should be embedded");
        let html = String::from_utf8(asset.data.into_owned())
            .expect("embedded index should be valid utf-8");

        assert!(html.contains("<div id=\"app\"></div>"));
        assert!(html.contains("/assets/index-"));
    }

    #[test]
    fn serves_referenced_assets_with_cache_headers() {
        let asset_path = FileShareWebAssets::iter()
            .find(|path| path.starts_with("assets/index-") && path.ends_with(".js"))
            .expect("built js asset should be embedded");

        let response = serve_asset(&asset_path).expect("embedded asset should be served");

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
}
