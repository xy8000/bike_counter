//! BFF map-tile proxy.
//!
//! `/api/map/*` forwards to the internal Martin tile server so the browser only
//! ever talks same-origin and the tile infrastructure stays behind the BFF
//! (Frontend -> BFF -> Martin). This is pure driving-adapter plumbing: no
//! core/domain code is involved. Tiles are intentionally served without any
//! caching header (the plan explicitly forbids caching).

use std::io::Read;

use axum::body::Body;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// Internal Martin tile server (docker service name + default port). Martin
/// serves each mounted archive under the id derived from its file stem
/// (extension-less), e.g. `/world/{z}/{x}/{y}` and `/basemap/{z}/{x}/{y}`.
const MARTIN_UPSTREAM: &str = "http://martin:3000";

/// Returns the upstream URL for a captured tile path, or `None` when the path
/// is not a safe tile request (open-proxy / path-traversal guard).
fn tile_upstream_url(path: &str) -> Option<String> {
    let safe = !path.is_empty()
        && !path.contains("..")
        && !path.contains('\\')
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'));
    safe.then(|| format!("{MARTIN_UPSTREAM}/{path}"))
}

/// Fetches a tile from `url` with the blocking `ureq` client. Tiles are small,
/// so buffering the body is fine and avoids a new async HTTP dependency. Called
/// from the blocking thread pool (see [`get_tile_proxy`]).
fn fetch_tile(url: &str) -> Result<(u16, Option<String>, Vec<u8>), String> {
    let response = ureq::get(url).call().map_err(|error| error.to_string())?;
    let status = response.status();
    let content_type = response.header("Content-Type").map(str::to_string);
    let mut bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok((status, content_type, bytes))
}

/// Proxies a tile request to Martin: validates the captured path, fetches the
/// tile on the blocking thread pool (matching the `spawn_blocking` pattern used
/// for the sync `postgres`/`ureq` calls) and streams the bytes back with the
/// upstream content type. No caching header is added — tiles pass through
/// uncached.
pub async fn get_tile_proxy(Path(path): Path<String>) -> Response {
    let Some(url) = tile_upstream_url(&path) else {
        return (StatusCode::BAD_REQUEST, "invalid tile path").into_response();
    };

    let result = tokio::task::spawn_blocking(move || fetch_tile(&url))
        .await
        .map_err(|join_error| join_error.to_string())
        .and_then(|inner| inner);

    match result {
        Ok((status, content_type, bytes)) => {
            let mut builder = Response::builder().status(status);
            if let Some(content_type) = content_type {
                builder = builder.header("Content-Type", content_type);
            }
            builder
                .body(Body::from(bytes))
                .expect("valid tile response")
        }
        Err(_) => (StatusCode::BAD_GATEWAY, "tile upstream unavailable").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::tile_upstream_url;

    #[test]
    fn maps_a_valid_tile_path_to_the_martin_upstream() {
        assert_eq!(
            tile_upstream_url("basemap/5/16/11"),
            Some("http://martin:3000/basemap/5/16/11".to_string())
        );
    }

    #[test]
    fn allows_nested_paths_with_extensions() {
        assert_eq!(
            tile_upstream_url("world/2/2/1.pbf"),
            Some("http://martin:3000/world/2/2/1.pbf".to_string())
        );
    }

    #[test]
    fn rejects_path_traversal_and_unsafe_characters() {
        for path in [
            "",
            "..",
            "basemap/../../etc/passwd",
            "basemap/..%2f..",
            "basemap\\5\\16\\11",
            "basemap/5/16/11?x=1",
            "basemap/5/16/11#frag",
        ] {
            assert_eq!(tile_upstream_url(path), None, "path: {path:?}");
        }
    }
}
