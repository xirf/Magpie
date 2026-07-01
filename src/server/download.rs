use axum::{
    extract::{Path as AxumPath, Query, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::path::Path;

use super::{AppState, serve_zipped_directory};
use super::index::serve_directory_index;
use super::file::serve_file;
use super::utils::render_error;

pub async fn handle_download(
    State(state): State<AppState>,
    AxumPath(params): AxumPath<std::collections::HashMap<String, String>>,
    Query(query): Query<std::collections::HashMap<String, String>>,
    request: Request,
) -> impl IntoResponse {
    let token = match params.get("token") {
        Some(t) => t,
        None => return (StatusCode::BAD_REQUEST, "Missing token").into_response(),
    };

    let base_path_str = match crate::db::get_local_download_path(token) {
        Some(p) => p,
        None => return render_error("Download Link Expired", "This download link has expired or is invalid. Please generate a new download link via the bot."),
    };

    let base_path = Path::new(&base_path_str);
    let subpath = params.get("subpath").map(|s| s.as_str()).unwrap_or("");
    let target_path = if subpath.is_empty() {
        base_path.to_path_buf()
    } else {
        let base_name = base_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let subpath_path = Path::new(subpath);
        let mut components = subpath_path.components();
        if let Some(first_comp) = components.next() {
            let first_comp_str = first_comp.as_os_str().to_str().unwrap_or("");
            if first_comp_str == base_name {
                let rest: std::path::PathBuf = components.collect();
                base_path.join(rest)
            } else {
                base_path.join(subpath)
            }
        } else {
            base_path.to_path_buf()
        }
    };

    println!(
        "[Server] Request for token: {}, subpath: {}",
        token, subpath
    );

    // Directory traversal security check
    let canonical_base = match base_path.canonicalize() {
        Ok(p) => {
            println!("[Server] canonical_base: {:?}", p);
            p
        }
        Err(e) => {
            eprintln!(
                "[Server] Failed to canonicalize base_path {:?}: {:?}",
                base_path, e
            );
            return render_error(
                "Not Found",
                "The requested download directory was not found on the server.",
            );
        }
    };
    let canonical_target = match target_path.canonicalize() {
        Ok(p) => {
            println!("[Server] canonical_target: {:?}", p);
            p
        }
        Err(e) => {
            eprintln!(
                "[Server] Failed to canonicalize target_path {:?}: {:?}",
                target_path, e
            );
            return render_error(
                "Not Found",
                "The requested file or directory does not exist.",
            );
        }
    };
    if !canonical_target.starts_with(&canonical_base) {
        return (
            StatusCode::FORBIDDEN,
            "Access Denied: Path Traversal Detected",
        )
            .into_response();
    }

    if canonical_target.is_dir() {
        let zip_requested = query
            .get("zip")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        if zip_requested {
            // Try to acquire the zipping permit to limit concurrent CPU/IO heavy tasks on the STB
            let permit = match state.zip_semaphore.clone().try_acquire_owned() {
                Ok(p) => p,
                Err(_) => {
                    return render_error(
                        "Server Busy",
                        "The server is currently zipping another download. Please wait a minute and try again to avoid overloading the system."
                    ).into_response();
                }
            };
            return serve_zipped_directory(&canonical_target, permit)
                .await
                .into_response();
        } else {
            return serve_directory_index(token, subpath, &canonical_base, &canonical_target)
                .await
                .into_response();
        }
    } else if canonical_target.is_file() {
        return serve_file(&canonical_target, request.headers())
            .await
            .into_response();
    }

    render_error("Not Found", "The requested object type is not supported.")
}