// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Control logging with systemd.

// zbus throws a fit about missing docs
#![allow(missing_docs)]

use std::convert::TryFrom;
use std::fmt::{Display, Formatter};

use tracing::debug;
use tracing::instrument;
use zbus::dbus_interface;

/// A syslog log level as used by the systemd log control interface.
#[derive(Debug, Copy, Clone)]
pub enum LogLevel {
    Emerg,
    Alert,
    Crit,
    Err,
    Warning,
    Notice,
    Info,
    Debug,
}

#[derive(Debug, Copy, Clone)]
pub enum LogLevelParseError {
    InvalidLogLevel,
}

impl TryFrom<&str> for LogLevel {
    type Error = LogLevelParseError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "emerg" => Ok(LogLevel::Emerg),
            "alert" => Ok(LogLevel::Alert),
            "crit" => Ok(LogLevel::Crit),
            "err" => Ok(LogLevel::Err),
            "warning" => Ok(LogLevel::Warning),
            "notice" => Ok(LogLevel::Notice),
            "info" => Ok(LogLevel::Info),
            "debug" => Ok(LogLevel::Debug),
            _ => Err(LogLevelParseError::InvalidLogLevel),
        }
    }
}

impl Display for LogLevel {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let level = match self {
            LogLevel::Emerg => "emerg",
            LogLevel::Alert => "alert",
            LogLevel::Crit => "crit",
            LogLevel::Err => "err",
            LogLevel::Warning => "warning",
            LogLevel::Notice => "notice",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
        };
        write!(f, "{}", level)
    }
}

/// Log targets used by the systemd log control interface.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum LogTarget {
    /// The regular console, i.e. stdout or stderr.
    Console,
    /// The kernel ring message buffer.
    ///
    /// Normally not used by userspace services.
    Kmsg,
    /// The direct interface to the systemd journal.
    ///
    /// Prefer this over `Syslog`, and over console logging,
    /// if the process runs under systemd, because this interface
    /// retains all structured data.
    Journal,
    /// The legacy syslog interface.
    ///
    /// Services which use systemd should prefer the `Journal` interface.
    Syslog,
    /// Disable all logging.
    Null,
    /// Automatically log to console or journal.
    ///
    /// If the stdout or stderr streams of the current process are
    /// connected to the systemd journal this is equivalent to `Journal`.
    /// Otherwise it's `Console`.
    Auto,
}

#[derive(Debug, Copy, Clone)]
pub enum LogTargetParseError {
    InvalidLogTarget,
}

impl TryFrom<&str> for LogTarget {
    type Error = LogTargetParseError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "console" => Ok(LogTarget::Console),
            "kmsg" => Ok(LogTarget::Kmsg),
            "journal" => Ok(LogTarget::Journal),
            "syslog" => Ok(LogTarget::Syslog),
            "null" => Ok(LogTarget::Null),
            "auto" => Ok(LogTarget::Auto),
            _ => Err(LogTargetParseError::InvalidLogTarget),
        }
    }
}

impl Display for LogTarget {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let target = match self {
            LogTarget::Console => "console",
            LogTarget::Kmsg => "kmsg",
            LogTarget::Journal => "journal",
            LogTarget::Syslog => "syslog",
            LogTarget::Null => "null",
            LogTarget::Auto => "auto",
        };
        write!(f, "{}", target)
    }
}

/// Control target and level of a log framework.
///
/// Bridges the log control DBus interface to an actual logging framework.
pub trait LogController: Send + Sync {
    /// Get the currently configured log level.
    fn level(&self) -> LogLevel;

    /// Set the level of the underlying log framework.
    fn set_level(&mut self, level: LogLevel) -> zbus::fdo::Result<()>;

    /// Get the currently configured log target.
    fn target(&self) -> LogTarget;

    /// Set the target of the underlying log framework.
    fn set_target(&mut self, target: LogTarget) -> zbus::fdo::Result<()>;
}

/// Control logging remotely.
pub struct LogControl {
    /// The log controller
    controller: Box<dyn LogController>,
}

impl LogControl {
    /// Create a new log control interface.
    ///
    /// `target` and `level` are the initial defaults for the configured log target and log level.
    ///
    /// Return the control interface, and a receiver for requests made by the control interface.  
    /// The caller needs to listen on this receiver for log configuration requests and update the
    /// logging state accordingly.
    pub(crate) fn new<T: LogController + 'static>(controller: T) -> Self {
        Self {
            controller: Box::new(controller),
        }
    }
}

/// The log control interface.
///
/// See <https://www.freedesktop.org/software/systemd/man/org.freedesktop.LogControl1.html>.
#[dbus_interface(name = "org.freedesktop.LogControl1")]
impl LogControl {
    /// Get the currently configured log level.
    #[dbus_interface(property)]
    fn log_level(&self) -> String {
        self.controller.level().to_string()
    }

    /// Set the new log level.
    #[dbus_interface(property)]
    #[instrument(skip(self))]
    async fn set_log_level(&mut self, level: String) -> zbus::fdo::Result<()> {
        let level = LogLevel::try_from(level.as_str())
            .map_err(|_| zbus::fdo::Error::InvalidArgs(format!("Unknown log level: {}", level)))?;
        debug!("Changing log level to {}", level);
        self.controller.set_level(level)
    }

    /// Get the currently configured log target.
    #[dbus_interface(property)]
    fn log_target(&self) -> String {
        self.controller.target().to_string()
    }

    /// Change the log target.
    #[dbus_interface(property)]
    #[instrument(skip(self))]
    async fn set_log_target(&mut self, target: String) -> zbus::fdo::Result<()> {
        let target = LogTarget::try_from(target.as_str()).map_err(|_| {
            zbus::fdo::Error::InvalidArgs(format!("Unknown log target: {}", target))
        })?;
        debug!("Changing log target to {}", target);
        self.controller.set_target(target)
    }

    /// Get the syslog identifier used by the service.
    #[dbus_interface(property)]
    #[instrument(skip(self))]
    async fn syslog_identifier(&self) -> String {
        unimplemented!()
    }
}
