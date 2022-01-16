// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Logging setup

use std::any::TypeId;
use std::default::Default;

use tracing::{debug, error, warn};
use tracing::{instrument, Subscriber};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format::Pretty;
use tracing_subscriber::layer::Layered;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::reload;
use tracing_subscriber::reload::Layer;
use tracing_subscriber::Registry;
use tracing_subscriber::{fmt, EnvFilter};

pub use crate::logcontrol::LogControl;
use crate::logcontrol::{LogController, LogLevel, LogTarget};

/// Log targets supported by tracing.
#[derive(Debug, Copy, Clone)]
enum TracingTarget {
    /// No logging.
    Null,
    /// Denotes the journal handler.
    Journal,
    /// Denotes the fmt handler, with pretty format on stdout.
    Console,
}

impl From<TracingTarget> for LogTarget {
    fn from(target: TracingTarget) -> Self {
        match target {
            TracingTarget::Null => LogTarget::Null,
            TracingTarget::Journal => LogTarget::Journal,
            TracingTarget::Console => LogTarget::Console,
        }
    }
}

// A bunch of type aliases to make the tracing types more tractable.
type JournalLayer = Option<tracing_journald::Layer>;
type StdoutLayer<S> = Option<fmt::Layer<S, fmt::format::Pretty, fmt::format::Format<Pretty>>>;
type TargetLayer<S> = Layered<StdoutLayer<S>, JournalLayer, S>;
type FilterSubscriber = Layered<Option<EnvFilter>, Registry>;
type LevelSubscriber = Layered<Layer<LevelFilter, FilterSubscriber>, FilterSubscriber>;

/// Create a new target lavel for the given `target`.
///
/// If `target` is `Null` enable no inner layer at all; otherwise enable the journal or
/// stdout inner layers depending on whether `target` is `Journal` or `Console` respectively.
///
/// Returns an error if `target` is `Journal` but journald is not reachable.
fn make_target_layer<S>(target: TracingTarget) -> std::io::Result<TargetLayer<S>>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let stdout = if let TracingTarget::Console = target {
        Some(tracing_subscriber::fmt::layer().pretty())
    } else {
        None
    };
    let journal = if let TracingTarget::Journal = target {
        tracing_journald::Layer::new().map(|l| Some(l.with_field_prefix(None)))
    } else {
        Ok(None)
    }?;
    Ok(tracing_subscriber::Layer::and_then(journal, stdout))
}

/// Control the tracing log framework/
struct TracingController {
    /// Whether the current process is connected to the systemd journal.
    connected_to_journal: bool,
    /// The current level active in the level layer.
    level: tracing::Level,
    /// The current target active in the target layer.
    target: TracingTarget,
    /// A handle to reload the level layer in order to change the level.
    level_handle: reload::Handle<LevelFilter, FilterSubscriber>,
    /// A handle to reload the target layer in order to change the target.
    target_handle: reload::Handle<TargetLayer<LevelSubscriber>, LevelSubscriber>,
}

/// Convert a systemd log level to a tracing log level.
///
/// Return an error if the systemd log level is not supported.
fn from_log_level(level: LogLevel) -> zbus::fdo::Result<tracing::Level> {
    match level {
        LogLevel::Emerg | LogLevel::Alert | LogLevel::Crit => {
            debug!("Rejecting unsupported level {}", level);
            Err(zbus::fdo::Error::NotSupported(format!(
                "Level {} not supported, {} is max level",
                level,
                LogLevel::Err
            )))
        }
        LogLevel::Err => Ok(tracing::Level::ERROR),
        LogLevel::Warning => Ok(tracing::Level::WARN),
        LogLevel::Notice => Ok(tracing::Level::INFO),
        LogLevel::Info => Ok(tracing::Level::DEBUG),
        LogLevel::Debug => Ok(tracing::Level::TRACE),
    }
}

/// Convert a tracing level to a systemd log level.
fn to_log_level(level: tracing::Level) -> LogLevel {
    match level {
        tracing::Level::ERROR => LogLevel::Err,
        tracing::Level::WARN => LogLevel::Warning,
        tracing::Level::INFO => LogLevel::Notice,
        tracing::Level::DEBUG => LogLevel::Info,
        tracing::Level::TRACE => LogLevel::Debug,
    }
}

impl LogController for TracingController {
    fn level(&self) -> LogLevel {
        to_log_level(self.level)
    }

    #[instrument(skip(self))]
    fn set_level(&mut self, level: LogLevel) -> zbus::fdo::Result<()> {
        debug!("Setting log level to {:?}", level);
        let tracing_level = from_log_level(level)?;
        self.level_handle
            .reload(LevelFilter::from_level(tracing_level))
            .map_err(|err| {
                error!("Failed to reload log level layer: {}", err);
                zbus::fdo::Error::Failed(format!("Failed to configure log level: {}", err))
            })?;
        self.level = tracing_level;
        debug!(
            "Successfully set log level to {}, mapped to tracing level {}",
            level, tracing_level
        );
        Ok(())
    }

    fn target(&self) -> LogTarget {
        self.target.into()
    }

    #[instrument(skip(self))]
    fn set_target(&mut self, target: LogTarget) -> zbus::fdo::Result<()> {
        debug!("Setting log target to {:?}", target);
        let tracing_target = match target {
            LogTarget::Kmsg | LogTarget::Syslog => {
                debug!("Rejecting unsupported target {}", target);
                Err(zbus::fdo::Error::NotSupported(format!(
                    "Log target {} not supported, use {}, {}, {} or {}",
                    target,
                    LogTarget::Auto,
                    LogTarget::Console,
                    LogTarget::Journal,
                    LogTarget::Null
                )))
            }
            LogTarget::Auto => {
                let desired_target = if self.connected_to_journal {
                    TracingTarget::Journal
                } else {
                    TracingTarget::Console
                };
                debug!(
                    "Using target {} for {}, connected to journal = {}",
                    LogTarget::from(desired_target),
                    target,
                    self.connected_to_journal
                );
                Ok(desired_target)
            }
            LogTarget::Journal => Ok(TracingTarget::Journal),
            LogTarget::Null => Ok(TracingTarget::Null),
            LogTarget::Console => Ok(TracingTarget::Console),
        }?;

        let target = make_target_layer(tracing_target).map_err(|err| {
            error!("Failed to connect to journald: {}", err);
            zbus::fdo::Error::Failed(format!("Failed to connect to journald: {}", err))
        })?;
        self.target_handle.reload(target).map_err(|err| {
            error!("Failed to reload target layer: {}", err);
            zbus::fdo::Error::Failed(format!("Failed to configure logging target: {}", err))
        })?;
        self.target = tracing_target;
        Ok(())
    }

    fn syslog_identifier(&self) -> String {
        self.target_handle
            .with_current(|layer| {
                unsafe { layer.downcast_raw(TypeId::of::<JournalLayer>()) }
                    .map(|raw| unsafe { &*(raw as *const JournalLayer) }.as_ref())
                    // We know that we've got a journal layer here, so we can definitely unwrap().
                    // If this fails it's definitely a bug!
                    .unwrap()
                    .map(|journal| journal.syslog_identifier().into())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    }
}

fn create_log_subscriber(connected_to_journal: bool) -> (impl Subscriber, TracingController) {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env().ok();

    let default_target = if connected_to_journal {
        TracingTarget::Journal
    } else {
        TracingTarget::Console
    };
    // If an env filter is set with $RUST_LOG use the lowest level as default for the control part,
    // to make sure the env filter takes precedence initially.
    let default_level = if env_filter.is_some() {
        tracing::Level::TRACE
    } else {
        tracing::Level::INFO
    };

    let (level, level_handle) = reload::Layer::new(LevelFilter::from_level(default_level));
    let (target, target_handle) = reload::Layer::new(make_target_layer(default_target).unwrap());

    let subscriber = Registry::default()
        .with(env_filter)
        .with(level)
        .with(target);

    let controller = TracingController {
        connected_to_journal,
        level: default_level,
        target: default_target,
        level_handle,
        target_handle,
    };

    (subscriber, controller)
}

/// Setup logging for a service.
///
/// If stdout or stderr are connected to the systemd journal setup direct
/// logging to journald; otherwise log to stdout.  The default level is info.
///
/// Return a `LogControl` struct which implements the systemd log control DBus
/// interface, see <https://www.freedesktop.org/software/systemd/man/org.freedesktop.LogControl1.html>.
///
/// This allows changing the log configuration at runtime with `systemctl service-log-level`.
pub fn setup_logging_for_service() -> LogControl {
    let (subscriber, controller) =
        create_log_subscriber(libsystemd::logging::connected_to_journal());

    // Setup tracing, and the redirect glib to log and log to tracing, to make sure everything
    // ends up in our log configuration.
    tracing::subscriber::set_global_default(subscriber).unwrap();
    glib::log_set_default_handler(glib::rust_log_handler);
    tracing_log::LogTracer::init().unwrap();

    LogControl::new(controller)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syslog_identifier_from_journal_layer() {
        let (subscriber, controller) = create_log_subscriber(true);
        assert!(!controller.syslog_identifier().is_empty());
        assert!(controller
            .syslog_identifier()
            .contains(env!("CARGO_CRATE_NAME")));
        // Make sure the subscriber is alive until after we test the syslog identifier,
        // because tracing otherwise tears down the layer.
        drop(subscriber)
    }

    #[test]
    fn syslog_identifier_with_journal_layer() {
        let (subscriber, controller) = create_log_subscriber(false);
        assert!(controller.syslog_identifier().is_empty());
        drop(subscriber)
    }
}
