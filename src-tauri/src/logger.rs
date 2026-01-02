use std::path::PathBuf;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{
    fmt,
    layer::{Layer, SubscriberExt},
    util::SubscriberInitExt,
    EnvFilter,
};

pub fn init_logger() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Configurar diretório de logs
    // Windows: C:\Users\User\AppData\Local\HipoCast\logs
    let log_dir = get_log_directory();
    std::fs::create_dir_all(&log_dir)?;

    println!("📝 Log directory: {:?}", log_dir);

    // 2. Configurar rotação de arquivos (Diária)
    let file_appender = RollingFileAppender::new(Rotation::DAILY, &log_dir, "hipocast.log");

    // 3. Configurar formato de tempo (Simples)
    // Usando formato padrão por enquanto para evitar problemas com time/OffsetTime
    let timer = fmt::time::UtcTime::rfc_3339();

    // 4. Configurar Filtro (Default: INFO)
    // Pode ser sobrescrito via variável de ambiente RUST_LOG
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // 5. Configurar Layers

    // Layer de Arquivo (JSON para fácil parsing ou texto estruturado)
    // Vamos usar texto legível por enquanto, mas com timestamp preciso
    let file_layer = fmt::layer()
        .with_file(true)
        .with_line_number(true)
        .with_writer(file_appender)
        .with_timer(timer.clone())
        .with_ansi(false) // Arquivos não devem ter códigos de cor ANSI
        .with_target(false); // Simplificar log removendo target excessivo

    // Layer de Console (Colorido e bonito para desenv)
    let console_layer = fmt::layer()
        .with_timer(timer)
        .with_writer(std::io::stdout)
        .with_filter(env_filter.clone()); // Console segue o filtro

    // 6. Inicializar Subscriber
    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(console_layer)
        .init();

    // Redirecionar logs da biblioteca padrão 'log' para o tracing
    // Isso captura logs do FFmpeg se ele usar a crate `log` (indiretamente) ou outras libs
    // Nota: LogTracer::init() não é estritamente necessário se as libs usarem tracing,
    // mas ajuda com compatibilidade.

    tracing::info!("🚀 Logging system initialized successfully");
    tracing::info!("📁 Logs are being written to: {:?}", log_dir);

    Ok(())
}

fn get_log_directory() -> PathBuf {
    use dirs::data_local_dir;

    data_local_dir()
        .map(|p| p.join("HipoCast").join("logs"))
        .unwrap_or_else(|| PathBuf::from("logs"))
}
