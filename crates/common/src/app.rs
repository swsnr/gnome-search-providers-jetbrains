// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Apps and their recent items.

use gio::prelude::*;
use glib::{MainContext, VariantDict};
use libc::pid_t;
use log::{debug, error, info, trace, warn};

use crate::systemd::{start_app_scope, ScopeProperties, Systemd1ManagerProxy};
use zbus::zvariant::OwnedObjectPath;

mod item;
mod launchservice;
mod provider;

pub use item::{AppLaunchItem, AppLaunchTarget};
pub use launchservice::{App, AppId, AppLaunchClient, AppLaunchRequest, AppLaunchService};
pub use provider::AppItemSearchProvider;

/// Settings for systemd scopes created by a search provider for launched apps.
#[derive(Debug, Clone)]
pub struct SystemdScopeSettings {
    /// The prefix for scope names.
    pub prefix: String,
    /// A string which identifies the app which started the scope.
    pub started_by: String,
    /// Optional documentation URLs for the scope.
    pub documentation: Vec<String>,
}

async fn move_launched_process_to_scope(
    connection: &zbus::Connection,
    id: &str,
    description: &str,
    scope_settings: SystemdScopeSettings,
    pid: pid_t,
) -> Result<(String, OwnedObjectPath), zbus::Error> {
    // Gnome also strips the .desktop suffix from IDs, see
    // https://gitlab.gnome.org/GNOME/gnome-desktop/-/blob/106a729c3f98b8ee56823a0a49fa8504f78dd355/libgnome-desktop/gnome-systemd.c#L227
    let properties = ScopeProperties {
        prefix: &scope_settings.prefix,
        name: id.trim_end_matches(".desktop"),
        description: Some(description),
        documentation: scope_settings
            .documentation
            .iter()
            .map(|v| v.as_str())
            .collect(),
    };
    trace!("Connect to Systemd1 Manager object");
    let systemd = Systemd1ManagerProxy::new(connection).await?;
    debug!(
        "Starting scope for process {} of app {} with properties: {:?}",
        pid, id, &properties
    );
    start_app_scope(&systemd, properties, pid).await
}

/// Create a launch context for apps.
///
/// The launch context moves every launched process into a new systemd scope to
/// isolate it from the search provider process.
///
/// It talks to the systemd Manager on `connection`, and uses the given settings to
/// fill the metadata of the new scope.
pub fn create_launch_context(
    connection: zbus::Connection,
    scope_settings: SystemdScopeSettings,
) -> gio::AppLaunchContext {
    let launch_context = gio::AppLaunchContext::new();
    launch_context.connect_launched(move |_, app, platform_data| {
        trace!(
            "App {} launched with platform_data: {:?}",
            app.id().unwrap(),
            platform_data
        );
        match platform_data
            .get::<VariantDict>()
            .and_then(|data| data.lookup_value("pid", None))
            .and_then(|value| value.get::<pid_t>())
        {
            None => warn!(
                "Failed to get PID of launched application from {:?}",
                platform_data
            ),
            Some(pid) => {
                let c = connection.clone();
                let s = scope_settings.clone();
                info!("App {} launched with PID {}", app.id().unwrap(), pid);
                let id = app.id().unwrap().to_string();
                let description = app.description().map_or_else(
                    || format!("app started by {}", scope_settings.started_by),
                    |value| format!("{} started by {}", value, scope_settings.started_by),
                );
                MainContext::ref_thread_default().spawn(async move  {
                    let result = move_launched_process_to_scope(
                        &c,
                        &id,
                        &description,
                        s,
                        pid,
                    )
                    .await;
                    match result {
                        Err(err) => error!("Failed to move running process {} of app {} into new systemd scope: {}",pid, id, err),
                        Ok((name, path)) => info!("Moved running process {} of app {} into new systemd scope {} at {}",pid,id,&name,path.into_inner()),
                    };
                })
            }
        }
    });
    launch_context
}

#[cfg(test)]
mod tests {
    mod search {
        use pretty_assertions::assert_eq;

        use crate::app::{AppLaunchItem, AppLaunchTarget};
        use crate::matching::find_matching_items;

        fn do_match<'a>(items: &[(&'a str, AppLaunchItem)], terms: &[&str]) -> Vec<&'a str> {
            find_matching_items(items.iter().map(|(s, p)| (*s, p)), terms)
        }

        #[test]
        fn matches_something() {
            let items = vec![(
                "foo",
                AppLaunchItem {
                    name: "mdcat".to_string(),
                    target: AppLaunchTarget::File("/home/foo/dev/mdcat".to_string()),
                },
            )];
            assert_eq!(do_match(&items, &["mdcat"]), ["foo"]);
        }

        /// Regression test for https://github.com/lunaryorn/gnome-search-providers-jetbrains/issues/7
        #[test]
        fn do_not_find_undesired_items() {
            let items = vec![
                (
                    "foo-1",
                    AppLaunchItem {
                        name: "ui-pattern-library".to_string(),
                        target: AppLaunchTarget::File(
                            "/home/foo/dev/something/ui-pattern-library".to_string(),
                        ),
                    },
                ),
                (
                    "foo-2",
                    AppLaunchItem {
                        name: "dauntless-builder".to_string(),
                        target: AppLaunchTarget::File(
                            "/home/foo/dev/dauntless-builder".to_string(),
                        ),
                    },
                ),
                (
                    "foo-3",
                    AppLaunchItem {
                        name: "typo3-ssr".to_string(),
                        target: AppLaunchTarget::File(
                            "/home/foo/dev/something/typo3-ssr".to_string(),
                        ),
                    },
                ),
            ];
            assert!(do_match(&items, &["flutter_test_app"]).is_empty());
        }

        #[test]
        fn ignore_case_of_name() {
            let items = vec![(
                "foo",
                AppLaunchItem {
                    name: "mdCat".to_string(),
                    target: AppLaunchTarget::File("/home/foo/dev/foo".to_string()),
                },
            )];
            assert_eq!(do_match(&items, &["Mdcat"]), ["foo"]);
        }

        #[test]
        fn ignore_case_of_path() {
            let items = vec![(
                "foo",
                AppLaunchItem {
                    name: "bar".to_string(),
                    target: AppLaunchTarget::File("/home/foo/dev/mdcaT".to_string()),
                },
            )];
            assert_eq!(do_match(&items, &["Mdcat"]), ["foo"]);
        }

        #[test]
        fn matches_in_name_rank_higher() {
            let items = vec![
                (
                    "1",
                    AppLaunchItem {
                        name: "bar".to_string(),
                        // This matches foo as well because of /home/foo
                        target: AppLaunchTarget::File("/home/foo/dev/bar".to_string()),
                    },
                ),
                (
                    "2",
                    AppLaunchItem {
                        name: "foo".to_string(),
                        target: AppLaunchTarget::File("/home/foo/dev/foo".to_string()),
                    },
                ),
            ];
            assert_eq!(do_match(&items, &["foo"]), ["2", "1"]);
        }

        #[test]
        fn matches_at_end_of_path_rank_higher() {
            let items = vec![
                (
                    "1",
                    AppLaunchItem {
                        name: "p1".to_string(),
                        // This matches foo as well because of /home/foo
                        target: AppLaunchTarget::File("/home/foo/dev/bar".to_string()),
                    },
                ),
                (
                    "2",
                    AppLaunchItem {
                        name: "p1".to_string(),
                        target: AppLaunchTarget::File("/home/foo/dev/foo".to_string()),
                    },
                ),
            ];
            assert_eq!(do_match(&items, &["foo"]), ["2", "1"]);
        }
    }
}
