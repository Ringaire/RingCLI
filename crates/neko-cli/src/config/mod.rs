use tracing_subscriber::EnvFilter;
use crate::args::Args;

pub fn setup_tracing(args: &Args) {
    let level = if args.verbose { "debug" } else if args.debug.is_some() { "debug" } else { "warn" };
    let filter = match &args.debug {
        Some(cat) => EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(format!("neko={level},neko_core={level},{cat}"))),
        None => EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(format!("neko={level},neko_core={level}"))),
    };

    let log_path = neko_core::session::paths::log_path();
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let file_appender = tracing_appender::rolling::never(
        log_path.parent().unwrap_or_else(|| std::path::Path::new(".")),
        log_path.file_name().unwrap_or_else(|| std::ffi::OsStr::new("neko.log")),
    );
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    // _guard must live for the process lifetime, leak it intentionally
    std::mem::forget(_guard);

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .init();
}
