use crate::core::config::Config;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{Arc, OnceLock, RwLock};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

const LOG_BUFFER_CAPACITY: usize = 1000;

/// One captured log line, exposed to the dashboard's Logs view.
#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

static LOG_BUFFER: OnceLock<Arc<RwLock<VecDeque<LogLine>>>> = OnceLock::new();

fn log_buffer() -> &'static Arc<RwLock<VecDeque<LogLine>>> {
    LOG_BUFFER.get_or_init(|| Arc::new(RwLock::new(VecDeque::new())))
}

/// Returns a copy of the recent log buffer (oldest first).
pub fn recent_logs() -> Vec<LogLine> {
    let buf = log_buffer().read().unwrap_or_else(|p| p.into_inner());
    buf.iter().cloned().collect()
}

struct MessageVisitor {
    message: String,
}

impl Visit for MessageVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        }
    }
}

/// Tracing layer that mirrors every event into the dashboard's in-memory
/// ring buffer. Bounded and best-effort — never blocks the logging path.
struct BufferLayer;

impl<S: Subscriber> Layer<S> for BufferLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MessageVisitor {
            message: String::new(),
        };
        event.record(&mut visitor);

        let line = LogLine {
            timestamp: now_rfc3339(),
            level: event.metadata().level().to_string(),
            target: event.metadata().target().to_string(),
            message: visitor.message,
        };

        let mut buf = log_buffer().write().unwrap_or_else(|p| p.into_inner());
        if buf.len() >= LOG_BUFFER_CAPACITY {
            buf.pop_front();
        }
        buf.push_back(line);
    }
}

/// RFC 3339 UTC timestamp without pulling in a date-time crate.
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let time_of_day = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60
    )
}

// Howard Hinnant's civil-from-days algorithm.
fn civil_from_days(z: i64) -> (i64, u64, u64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

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

    let fmt_layer = fmt::layer().with_ansi(config.logger.ansi).with_file(true).with_line_number(true);

    let sentry_layer = if config.enable_telemetry && sentry_guard.is_some() {
        Some(sentry_tracing::layer())
    } else {
        None
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(sentry_layer)
        .with(BufferLayer)
        .init();

    sentry_guard
}
