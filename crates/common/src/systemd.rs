// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Systemd utilities.

use libc::{dev_t, ino_t, pid_t};
use slog::{debug, trace, Logger};
use std::os::unix::io::{AsRawFd, RawFd};
use std::str::FromStr;
use zbus::dbus_proxy;
use zbus::export::zvariant::{OwnedObjectPath, Value};

fn fd_has_device_and_inode(fd: RawFd, device: dev_t, inode: ino_t) -> bool {
    nix::sys::stat::fstat(fd).map_or(false, |stat| stat.st_dev == device && stat.st_ino == inode)
}

/// Whether the standard error of this process is connected to the systemd journal.
///
/// Checks whether `$JOURNAL_STREAM` is set and non-empty, and points to stdout or stderr.
///
/// See [systemd.exec][1] for more information.
///
/// [1]: https://www.freedesktop.org/software/systemd/man/systemd.exec.html#Environment%20Variables%20in%20Spawned%20Processes
pub fn connected_to_journal() -> bool {
    std::env::var_os("JOURNAL_STREAM")
        .as_ref()
        .and_then(|value| value.to_str())
        .and_then(|value| value.split_once(':'))
        .and_then(|(device, inode)| u64::from_str(device).ok().zip(u64::from_str(inode).ok()))
        .map_or(false, |(device, inode)| {
            fd_has_device_and_inode(std::io::stderr().as_raw_fd(), device, inode)
                || fd_has_device_and_inode(std::io::stdout().as_raw_fd(), device, inode)
        })
}

/// The systemd manager DBUS API.
///
/// See <https://www.freedesktop.org/wiki/Software/systemd/dbus/>
#[dbus_proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
pub trait Systemd1Manager {
    /// StartTransientUnit() may be used to create and start a transient unit,
    /// which will be released as soon as it is not running or referenced anymore or the system is rebooted.
    ///
    /// `name` is the unit name including suffix, and must be unique. `mode` is the same as in `StartUnit()`,
    /// `properties` contains properties of the unit, specified like in `SetUnitProperties()`.
    /// `aux` is currently unused and should be passed as empty array.
    fn start_transient_unit(
        &self,
        name: &str,
        mode: &str,
        properties: &[(&str, Value<'_>)],
        aux: &[(&str, Vec<(&str, Value<'_>)>)],
    ) -> zbus::Result<OwnedObjectPath>;
}

/// Properties for a new systemd scope.
#[derive(Debug)]
pub struct ScopeProperties<'a> {
    /// The prefix to prepend before the scope name.
    ///
    /// This string is prepended **literally**, and not escaped.
    ///
    /// It must be a valid string for a systemd unit.
    pub prefix: &'a str,
    /// The name for the scope.
    ///
    /// This string gets escaped for systemd and can be arbitrary.
    pub name: &'a str,
    /// The optional description for the unit.
    ///
    /// This is used by systemd (and other UIs) as the label for the unit,
    /// so this string should identify the unit rather than describe it, despite the name.
    ///
    /// See `systemd.unit(5)` for more information.
    pub description: Option<&'a str>,
    /// The optional documentation URLs for the unit.
    pub documentation: Vec<&'a str>,
}

/// Escape a string for use in a systemd unit name.
///
/// See <https://www.freedesktop.org/software/systemd/man/systemd.unit.html#String%20Escaping%20for%20Inclusion%20in%20Unit%20Names>
fn escape_name(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len() * 2);
    for (index, b) in s.bytes().enumerate() {
        match b {
            b'/' => escaped.push('-'),
            // Do not escape '.' unless it's the first character
            b'.' if 0 < index => escaped.push(char::from(b)),
            // Do not escaoe _ and : and
            b'_' | b':' => escaped.push(char::from(b)),
            // all ASCII alpha numeric characters
            _ if b.is_ascii_alphanumeric() => escaped.push(char::from(b)),
            _ => escaped.push_str(&format!("\\x{:02x}", b)),
        }
    }
    escaped
}

/// The systemd manager on DBus.
pub struct Systemd1Manager {
    logger: Logger,
    proxy: Systemd1ManagerProxy<'static>,
}

impl Systemd1Manager {
    /// Connect to the systemd manager on the given `connection`.
    ///
    /// Use the given `logger` for logging.
    pub fn new(logger: Logger, connection: &zbus::Connection) -> zbus::Result<Self> {
        debug!(logger, "Connecting to systemd on {:?}", connection);
        Systemd1ManagerProxy::new(connection).map(|proxy| Self { logger, proxy })
    }

    /// Start a new systemd application scope for a running process.
    ///
    /// `properties` provides the name and the metadata for the new scope.
    ///
    /// `pid` is the process ID of the process to move into a new scope.
    ///
    /// Return the complete name and the DBUS object path of the new scope unit if successful.
    pub fn start_app_scope(
        &self,
        properties: ScopeProperties,
        pid: pid_t,
    ) -> zbus::Result<(String, OwnedObjectPath)> {
        trace!(self.logger, "start_app_scope({:?}, {})", properties, pid);
        // See https://gitlab.gnome.org/jf/start-transient-unit/-/blob/117c6f32c8dc0d1f28686408f698632aa71880bc/rust/src/main.rs#L94
        // for inspiration.
        //
        // See https://www.freedesktop.org/wiki/Software/systemd/ControlGroupInterface/ for background.
        let mut props = vec![
            // I haven't found any documentation for the type of the PIDs property, but
            // systemd appears to use u32 for PIDs, even though pid_t is a signed type.
            // libgnome also uses uint32, see
            // https://gitlab.gnome.org/GNOME/gnome-desktop/-/blob/106a729c3f98b8ee56823a0a49fa8504f78dd355/libgnome-desktop/gnome-systemd.c#L94
            //
            // Attempting to pass PID as pid_t directly results in a ENXIO error from systemd.
            ("PIDs", Value::Array(vec![pid as u32].into())),
            // libgnome passes this property too, see
            // https://gitlab.gnome.org/GNOME/gnome-desktop/-/blob/106a729c3f98b8ee56823a0a49fa8504f78dd355/libgnome-desktop/gnome-systemd.c#L100
            //
            // I'm not entirely sure how it's relevant but it seems a good idea to do what Gnome does.
            ("CollectMode", Value::Str("inactive-or-failed".into())),
        ];
        if let Some(description) = properties.description {
            props.push(("Description", Value::Str(description.into())));
        }
        if !properties.documentation.is_empty() {
            props.push((
                "Documentation",
                Value::Array(properties.documentation.into()),
            ))
        }
        // This is roughly what Gnome itself does when it moves a new process to a systemd scope, see
        // https://gitlab.gnome.org/GNOME/gnome-desktop/-/blob/106a729c3f98b8ee56823a0a49fa8504f78dd355/libgnome-desktop/gnome-systemd.c#L81
        //
        // Gnome Shell uses a the app-gnome prefix; we make the prefix configurable to allow callers to identify their new scopes.
        let name = format!(
            "{}-{}-{}.scope",
            properties.prefix,
            escape_name(&properties.name),
            pid
        );
        debug!(self.logger, "Creating new scope {} for {}", &name, pid);
        // We `fail` to start the scope if it already exists.
        let result = self
            .proxy
            .start_transient_unit(&name, "fail", &props, &Vec::new());
        trace!(
            self.logger,
            "StartTransientUnit({}, fail, {:?}, []) -> {:?}",
            &name,
            &props,
            &result,
        );
        result.map(|objpath| (name, objpath))
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    #[test]
    fn escape_name() {
        let samples = vec![
            // (input, escaped)
            ("test", "test"),
            ("a:b_c.d", "a:b_c.d"),
            ("/foo/", "-foo-"),
            (".foo", "\\x2efoo"),
            ("Hallöchen, Meister", "Hall\\xc3\\xb6chen\\x2c\\x20Meister"),
        ];

        for (input, expected) in samples {
            assert_eq!(super::escape_name(input), expected);
        }
    }
}
