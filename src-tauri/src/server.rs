use axum::{
    Router,
    response::IntoResponse,
    http::header,
};
use tower_http::services::ServeDir;
use tower_http::cors::CorsLayer;
use std::net::SocketAddr;
use tokio::net::TcpListener;

pub async fn start_server(port: u16, static_dir: String) {
    let streams_path = std::path::Path::new(&static_dir).to_path_buf();
    
    println!("Starting HLS server on port {} serving from {:?}", port, streams_path);
    
    let app = Router::new()
        .nest_service(
            "/streams",
            ServeDir::new(streams_path)
                .append_index_html_on_directories(false)
        )
        .layer(axum::middleware::from_fn(add_cache_headers))
        .layer(CorsLayer::permissive());

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("HLS Server listening on {}", addr);
    
    match TcpListener::bind(addr).await {
        Ok(listener) => {
            println!("✅ HLS Server successfully bound to {}", addr);
            if let Err(e) = axum::serve(listener, app).await {
                eprintln!("❌ HLS Server error: {}", e);
            }
        }
        Err(e) => {
            eprintln!("❌ Failed to bind HLS server to {}: {}", addr, e);
            eprintln!("   The port is likely already in use. Server will not start.");
            eprintln!("   Streams can still be created but won't be accessible via HTTP.");
        }
    }
}

async fn add_cache_headers(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> impl IntoResponse {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    
    headers.insert(header::CACHE_CONTROL, header::HeaderValue::from_static("no-cache, no-store, must-revalidate"));
    headers.insert(header::PRAGMA, header::HeaderValue::from_static("no-cache"));
    headers.insert(header::EXPIRES, header::HeaderValue::from_static("0"));
    
    response
}
