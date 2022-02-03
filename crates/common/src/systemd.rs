// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Systemd utilities.

use libc::pid_t;
use tracing::{debug, trace};
use zbus::dbus_proxy;
use zbus::zvariant::{OwnedObjectPath, Value};

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

/// Escape a systemd unit name.
///
/// See section "STRING ESCAPING FOR INCLUSION IN UNIT NAMES" in `systemd.unit(5)`
/// for details about the algorithm.
fn escape_name(name: &str) -> String {
    if name.is_empty() {
        "".to_string()
    } else {
        name.bytes()
            .enumerate()
            .map(|(n, b)| {
                let c = char::from(b);
                match c {
                    '/' => '-'.to_string(),
                    ':' | '_' | '0'..='9' | 'a'..='z' | 'A'..='Z' => c.to_string(),
                    '.' if n > 0 => c.to_string(),
                    _ => format!(r#"\x{:02x}"#, b),
                }
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

/// Start a new systemd application scope for a running process.
///
/// `properties` provides the name and the metadata for the new scope.
///
/// `pid` is the process ID of the process to move into a new scope.
///
/// Return the complete name and the DBUS object path of the new scope unit if successful.
pub async fn start_app_scope(
    manager: &Systemd1ManagerProxy<'_>,
    properties: ScopeProperties<'_>,
    pid: pid_t,
) -> zbus::Result<(String, OwnedObjectPath)> {
    // See https://gitlab.gnome.org/jf/start-transient-unit/-/blob/117c6f32c8dc0d1f28686408f698632aa71880bc/rust/src/main.rs#L94
    // for inspiration.
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
        escape_name(properties.name),
        pid
    );
    debug!("Creating new scope {} for {}", &name, pid);

    // We `fail` to start the scope if it already exists.
    let mode = "fail";
    let aux = &[];
    trace!(
        "StartTransientUnit({}, {}, {:?}, {:?})",
        name,
        mode,
        props,
        aux
    );
    let result = manager.start_transient_unit(&name, mode, &props, aux).await;
    trace!(
        "StartTransientUnit({}, {}, {:?}, []) -> {:?}",
        name,
        mode,
        props,
        result
    );
    result.map(|objpath| (name, objpath))
}
