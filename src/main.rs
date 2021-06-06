use tracing_subscriber::{fmt, prelude::*, EnvFilter};

const LOG_FILENAME: &str = "log";

fn main() {
    let term_logger = fmt::layer();
    let file_appender = tracing_appender::rolling::never(".", LOG_FILENAME);
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    let file_logger = fmt::layer().with_ansi(false).with_writer(non_blocking);

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::from("info")))
        .with(term_logger)
        .with(file_logger)
        .init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    #[cfg(feature = "discord")]
    runtime.block_on(bernbot::discord::main());
    #[cfg(all(feature = "harmony", not(feature = "discord")))]
    runtime.block_on(bernbot::harmony::main()).unwrap();

    runtime.shutdown_background();
}
