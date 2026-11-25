use axum::{http::header, response::IntoResponse, Router};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener as StdTcpListener, TcpStream};
use std::path::PathBuf;
use std::time::Duration;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

pub fn start_server(port: u16, streams_path: PathBuf) -> Result<u16, String> {
    let listener = StdTcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .map_err(|error| format!("Não foi possível abrir a porta HLS {port}: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();

    let app = Router::new()
        .nest_service(
            "/streams",
            ServeDir::new(streams_path).append_index_html_on_directories(false),
        )
        .layer(axum::middleware::from_fn(add_cache_headers))
        .layer(CorsLayer::permissive());

    tauri::async_runtime::spawn(async move {
        match TcpListener::from_std(listener) {
            Ok(listener) => {
                tracing::info!(port, "Servidor HLS disponível");
                if let Err(error) = axum::serve(listener, app).await {
                    tracing::error!(%error, "Falha no servidor HLS");
                }
            }
            Err(error) => tracing::error!(%error, "Falha no servidor HLS"),
        }
    });
    Ok(port)
}

pub fn playlist_is_served(port: u16, route: &str) -> bool {
    let check = || -> std::io::Result<bool> {
        let timeout = Duration::from_millis(250);
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let mut stream = TcpStream::connect_timeout(&address, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        write!(
            stream,
            "HEAD {route} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
        )?;
        let mut response = String::new();
        BufReader::new(stream).read_line(&mut response)?;
        Ok(response.starts_with("HTTP/1.1 200 "))
    };
    check().unwrap_or(false)
}

async fn add_cache_headers(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> impl IntoResponse {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();

    headers.insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-cache, no-store, must-revalidate"),
    );
    headers.insert(header::PRAGMA, header::HeaderValue::from_static("no-cache"));
    headers.insert(header::EXPIRES, header::HeaderValue::from_static("0"));

    response
}
