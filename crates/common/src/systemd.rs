// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Systemd utilities.

use libc::pid_t;
use zbus::dbus_proxy;
use zbus::export::zvariant::{OwnedObjectPath, Value};

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
        properties: Vec<(&str, Value<'_>)>,
        aux: Vec<(&str, Vec<(&str, Value<'_>)>)>,
    ) -> zbus::Result<OwnedObjectPath>;
}

/// Extensions to the systemd1 Manager API.
pub trait Systemd1ManagerExt {
    /// Start a new systemd application scope for a running process.
    ///
    /// `name` is the name for the new scope; it gets combined with a prefix and
    /// the `pid` to make a unique scope name.
    ///
    /// `description` is a human-readable string to use as description for the new scope which will
    /// appear e.g. in `systemctl` output.
    ///
    /// `pid` is the process ID of the process to move into a new scope.
    ///
    /// Return the complete name and the DBUS object path of the new scope unit if successful.
    fn start_app_scope(
        &self,
        name: &str,
        description: Option<&str>,
        pid: pid_t,
    ) -> zbus::Result<(String, OwnedObjectPath)>;
}

/// Escape a string for use in a systemd unit name.
///
/// See <https://www.freedesktop.org/software/systemd/man/systemd.unit.html#String%20Escaping%20for%20Inclusion%20in%20Unit%20Names>
fn systemd_escape(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'/' => escaped.push('-'),
            // _ is not escaped, and '.' only at the beginning (see below).
            // Not sure about :, see https://github.com/systemd/systemd/issues/19623
            b'.' | b'_' | b':' => escaped.push(b as char),
            // ASCII alpha numberic chars are not escaped
            _ if b.is_ascii_alphanumeric() => escaped.push(b as char),
            _ => escaped.push_str(&format!("\\x{:02x}", b)),
        }
    }
    // systemd requires a leading dot (and only a leading dot) to be escaped.
    if escaped.starts_with('.') {
        escaped.replacen('.', "\\x2e", 1)
    } else {
        escaped
    }
}

impl Systemd1ManagerExt for Systemd1ManagerProxy<'_> {
    // See https://gitlab.gnome.org/jf/start-transient-unit/-/blob/117c6f32c8dc0d1f28686408f698632aa71880bc/rust/src/main.rs#L94
    // for inspiration.
    fn start_app_scope(
        &self,
        name: &str,
        description: Option<&str>,
        pid: pid_t,
    ) -> zbus::Result<(String, OwnedObjectPath)> {
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
        if let Some(description) = description {
            props.push(("Description", Value::Str(description.into())))
        }
        // This is roughly what Gnome itself does when it moves a new process to a systemd scope, see
        // https://gitlab.gnome.org/GNOME/gnome-desktop/-/blob/106a729c3f98b8ee56823a0a49fa8504f78dd355/libgnome-desktop/gnome-systemd.c#L81
        let name = format!("app-gnome-{}-{}.scope", systemd_escape(name), pid);
        // We `fail` to start the scope if it already exists.
        self.start_transient_unit(&name, "fail", props, Vec::new())
            .map(|objpath| (name, objpath))
    }
}

#[cfg(test)]
mod tests {

    mod systemd_escape {
        use crate::systemd::systemd_escape;

        use pretty_assertions::assert_eq;

        fn expected_escaped(s: &str) -> std::io::Result<String> {
            use std::process::Command;
            let output = Command::new("systemd-escape").arg(s).output()?;
            assert!(output.status.success());
            Ok(String::from_utf8(output.stdout)
                .expect("systemd-escape returned non-UTF-8 data")
                .trim_end()
                .to_string())
        }

        #[test]
        fn ascii_unescaped() {
            assert_eq!(systemd_escape("test"), "test");
        }

        #[test]
        fn escape_non_ascii() {
            assert_eq!(
                systemd_escape("Hallöchen, Meister"),
                expected_escaped("Hallöchen, Meister").unwrap()
            );
        }

        #[test]
        fn escape_colon() {
            // See https://github.com/systemd/systemd/issues/19623
            assert_eq!(systemd_escape("a:b"), expected_escaped("a:b").unwrap());
        }

        #[test]
        fn escape_leading_dot() {
            assert_eq!(systemd_escape(".foo"), expected_escaped(".foo").unwrap());
        }
    }
}
