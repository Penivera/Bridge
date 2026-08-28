use tracing_subscriber::{
    fmt,
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
};
use crate::core::config::Config;

/// Initializes tracing logging and optional Sentry telemetry based on the configuration.
///
/// Returns an optional [`sentry::ClientInitGuard`] that should be held in `main`
/// for the lifetime of the process to ensure Sentry flushes all events on shutdown.
pub fn init_telemetry(config: &Config) -> Option<sentry::ClientInitGuard> {
    // 1. Initialize Sentry client if telemetry is enabled and DSN is available
    let sentry_guard = if config.enable_telemetry {
        let dsn = config
            .sentry
            .dsn
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or(option_env!("SENTRY_DSN"));

        dsn.map(|dsn_str| {
            sentry::init((
                dsn_str,
                sentry::ClientOptions {
                    release: config
                        .sentry
                        .release
                        .clone()
                        .map(Into::into)
                        .or_else(|| sentry::release_name!()),
                    environment: config.sentry.environment.clone().map(Into::into),
                    sample_rate: config.sentry.sample_rate,
                    traces_sample_rate: config.sentry.traces_sample_rate,
                    debug: config.sentry.debug,
                    ..Default::default()
                },
            ))
        })
    } else {
        None
    };

    // 2. Build tracing subscriber layers
    let filter = EnvFilter::builder()
        .with_default_directive(config.logger.level.into())
        .from_env_lossy();

    let fmt_layer = fmt::layer().with_ansi(config.logger.ansi)
        .with_file(true);

    let sentry_layer = if config.enable_telemetry && sentry_guard.is_some() {
        Some(sentry_tracing::layer())
    } else {
        None
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(sentry_layer)
        .init();

    sentry_guard
}
