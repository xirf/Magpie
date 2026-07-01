use axum::{
    routing::get,
    Router,
};
use std::sync::Arc;
use tokio::sync::RwLock;

pub mod download;
pub mod file;
pub mod index;
pub mod utils;
pub mod zip;

// Re-export key handlers
pub use download::handle_download;
pub use zip::serve_zipped_directory;

#[derive(Clone)]
pub struct AppState {
    pub _settings: Arc<RwLock<crate::config::Settings>>,
    pub zip_semaphore: Arc<tokio::sync::Semaphore>,
}

pub struct ChannelStream {
    pub receiver: tokio::sync::mpsc::Receiver<Result<bytes::Bytes, std::io::Error>>,
    pub _permit: Option<tokio::sync::OwnedSemaphorePermit>,
}

impl futures::Stream for ChannelStream {
    type Item = Result<bytes::Bytes, std::io::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

pub async fn start_server(settings: Arc<RwLock<crate::config::Settings>>) {
    let bind_addr = {
        let s = settings.read().await;
        s.local_server
            .bind_addr
            .clone()
            .unwrap_or_else(|| "0.0.0.0:3000".to_string())
    };

    println!("[Server] Starting embedded web server on {}...", bind_addr);

    let app = Router::new()
        .route("/download/:token", get(handle_download))
        .route("/download/:token/*subpath", get(handle_download))
        .with_state(AppState {
            _settings: settings,
            zip_semaphore: Arc::new(tokio::sync::Semaphore::new(1)), // Max 1 concurrent zip operation to protect STB resources
        });

    let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[Server] Failed to bind server to {}: {:?}", bind_addr, e);
            return;
        }
    };

    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("[Server] Web server execution error: {:?}", e);
    }
}
