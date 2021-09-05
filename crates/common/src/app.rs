// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Items to be launched by an app.

use std::collections::HashMap;

use gio::prelude::*;
use glib::{MainContext, VariantDict};
use indexmap::IndexMap;
use libc::pid_t;
use log::{debug, error, info, trace, warn};
use zbus::dbus_interface;
use zbus::export::zvariant;

use crate::matching::*;
use crate::systemd::{start_app_scope, AsyncSystemd1ManagerProxy, ScopeProperties};
use zbus::export::zvariant::OwnedObjectPath;

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
    connection: &zbus::azync::Connection,
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
    let systemd = AsyncSystemd1ManagerProxy::new(connection).await?;
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
                        c.inner(),
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

/// A target for launching an app.
#[derive(Debug, PartialEq)]
pub enum AppLaunchTarget {
    /// A URI to launch.
    Uri(String),
    /// A file to launch.
    File(String),
}

impl AppLaunchTarget {
    /// The description for this launch target.
    fn description(&self) -> &str {
        match self {
            AppLaunchTarget::Uri(uri) => uri,
            AppLaunchTarget::File(path) => path,
        }
    }
}

/// A recent item from the file system.
#[derive(Debug, PartialEq)]
pub struct AppLaunchItem {
    /// The human readable name
    pub name: String,

    /// The target to launch when activating this item.
    pub target: AppLaunchTarget,
}

impl ScoreMatchable for AppLaunchItem {
    /// Compute the score of matching self against `terms`.
    ///
    /// If all terms match the name each term contributes a score of 10; this makes sure
    /// that precise matches in the name boost the score somewhat to the top.
    ///
    /// If all terms match the target each term contributes 1 to score, scaled by the relative position
    /// of the right-most match, assuming that paths typically go from least to most specific segment,
    /// to the farther to the right a term matches the more specific it was.
    fn match_score<S: AsRef<str>>(&self, terms: &[S]) -> f64 {
        let name = self.name.to_lowercase();
        let target = match &self.target {
            AppLaunchTarget::Uri(uri) => uri,
            AppLaunchTarget::File(file) => file,
        }
        .to_lowercase();
        let name_score = terms.iter().try_fold(0.0, |score, term| {
            name.contains(&term.as_ref().to_lowercase())
                .then(|| score + 10.0)
                .ok_or(())
        });
        let target = terms.iter().try_fold(0.0, |score, term| {
            target
                .rfind(&term.as_ref().to_lowercase())
                .ok_or(())
                .map(|index| score + 1.0 * (index as f64 / target.len() as f64))
        });
        let score = name_score.unwrap_or_default() + target.unwrap_or_default();
        trace!(
            "Item {:?} matches terms {:?} with score {} (name={:?}, target={:?})",
            self,
            terms.iter().map(|s| s.as_ref()).collect::<Vec<&str>>(),
            score,
            name_score,
            target
        );
        score
    }
}

/// A search provider for recent items.
pub struct AppItemSearchProvider<S: ItemsSource<AppLaunchItem>> {
    launch_context: gio::AppLaunchContext,
    app: gio::DesktopAppInfo,
    source: S,
    items: IdMap<AppLaunchItem>,
}

impl<S: ItemsSource<AppLaunchItem>> AppItemSearchProvider<S> {
    /// Create a new search provider for recent items of `app`.
    ///
    /// Uses the given `source` to load recent items.
    pub fn new(app: gio::DesktopAppInfo, source: S, launch_context: gio::AppLaunchContext) -> Self {
        Self {
            launch_context,
            app,
            source,
            items: IndexMap::new(),
        }
    }
}

/// The DBus interface of the search provider.
///
/// See <https://developer.gnome.org/SearchProvider/> for information.
#[dbus_interface(name = "org.gnome.Shell.SearchProvider2")]
impl<S: ItemsSource<AppLaunchItem> + 'static> AppItemSearchProvider<S> {
    /// Starts a search.
    ///
    /// This function is called when a new search is started. It gets an array of search terms as arguments,
    /// and should return an array of result IDs. gnome-shell will call GetResultMetas for (some) of these result
    /// IDs to get details about the result that can be be displayed in the result list.
    fn get_initial_result_set(&mut self, terms: Vec<&str>) -> zbus::fdo::Result<Vec<String>> {
        trace!("Enter GetInitialResultSet({:?}", &terms);
        debug!("Searching for {:?} of {}", terms, self.app.id().unwrap());
        self.items = self.source.find_recent_items().map_err(|error| {
            error!(
                "Failed to update recent items for {}: {:#}",
                self.app.id().unwrap().as_str(),
                error,
            );
            zbus::fdo::Error::Failed(format!(
                "Failed to update recent items for {}: {:#}",
                self.app.id().unwrap(),
                error
            ))
        })?;

        let ids = find_matching_items(self.items.iter(), terms.as_slice())
            .into_iter()
            .map(String::to_owned)
            .collect();
        debug!(
            "Found ids {:?} for {}",
            ids,
            self.app.id().unwrap().as_str(),
        );
        trace!("GetInitialResultSet({:?} -> {:?}", &terms, &ids);
        Ok(ids)
    }

    /// Refine an ongoing search.
    ///
    /// This function is called to refine the initial search results when the user types more characters in the search entry.
    /// It gets the previous search results and the current search terms as arguments, and should return an array of result IDs,
    /// just like GetInitialResulSet.
    fn get_subsearch_result_set(
        &self,
        previous_results: Vec<&str>,
        terms: Vec<&str>,
    ) -> Vec<String> {
        trace!(
            "Enter GetSubsearchResultSet({:?}, {:?})",
            previous_results,
            terms
        );
        debug!(
            "Searching for {:?} in {:?} of {}",
            terms,
            previous_results,
            self.app.id().unwrap()
        );
        let candidates = previous_results
            .iter()
            .filter_map(|&id| self.items.get(id).map(|p| (id, p)));

        let ids = find_matching_items(candidates, terms.as_slice())
            .into_iter()
            .map(|s| s.to_owned())
            .collect();
        debug!(
            "Found ids {:?} for {}",
            ids,
            self.app.id().unwrap().as_str()
        );
        trace!(
            "GetSubsearchResultSet({:?}, {:?}) -> {:?}",
            previous_results,
            terms,
            ids
        );
        ids
    }

    /// Get metadata for results.
    ///
    /// This function is called to obtain detailed information for results.
    /// It gets an array of result IDs as arguments, and should return a matching array of dictionaries
    /// (ie one a{sv} for each passed-in result ID).
    ///
    /// The following pieces of information should be provided for each result:
    //
    //  - "id": the result ID
    //  - "name": the display name for the result
    //  - "icon": a serialized GIcon (see g_icon_serialize()), or alternatively,
    //  - "gicon": a textual representation of a GIcon (see g_icon_to_string()), or alternativly,
    //  - "icon-data": a tuple of type (iiibiiay) describing a pixbuf with width, height, rowstride, has-alpha, bits-per-sample, and image data
    //  - "description": an optional short description (1-2 lines)
    fn get_result_metas(&self, results: Vec<String>) -> Vec<HashMap<String, zvariant::Value>> {
        trace!("Enter GetResultMetas({:?}", results);
        debug!("Getting meta info for {:?}", results);
        let metas = results
            .iter()
            .filter_map(|id| {
                self.items.get(id).map(|item| {
                    debug!("Compiling meta info for {}", id);
                    let icon = IconExt::to_string(&self.app.icon().unwrap()).unwrap();
                    debug!("Using icon {} for id {}", icon, id);

                    let mut meta: HashMap<String, zvariant::Value> = HashMap::new();
                    meta.insert("id".to_string(), id.clone().into());
                    meta.insert("name".to_string(), (&item.name).into());
                    meta.insert("gicon".to_string(), icon.to_string().into());
                    meta.insert("description".to_string(), item.target.description().into());
                    meta
                })
            })
            .collect();

        trace!("GetResultMetas({:?} -> {:?}", results, &metas);
        metas
    }

    /// Activate an individual result.
    ///
    /// This function is called when the user clicks on an individual result to open it in the application.
    /// The arguments are the result ID, the current search terms and a timestamp.
    ///
    /// Launches the underlying app with the path to the selected item.
    fn activate_result(&self, id: &str, terms: Vec<&str>, timestamp: u32) -> zbus::fdo::Result<()> {
        trace!("Enter ActivateResult({}, {:?}, {})", id, terms, timestamp);
        debug!("Activating result {} for {:?} at {}", id, terms, timestamp);
        let result = if let Some(item) = self.items.get(id) {
            info!(
                "Launching recent item {:?} for {}",
                item,
                self.app.id().unwrap().as_str()
            );
            match &item.target {
                AppLaunchTarget::File(path) => self.app.launch::<gio::AppLaunchContext>(
                    &[gio::File::for_path(path)],
                    Some(&self.launch_context),
                ),
                AppLaunchTarget::Uri(uri) => {
                    self.app.launch_uris(&[uri], Some(&self.launch_context))
                }
            }
            .map_err(|error| {
                error!(
                    "Failed to launch app {} for target {}: {}",
                    self.app.id().unwrap(),
                    item.target.description(),
                    error,
                );
                zbus::fdo::Error::SpawnFailed(format!(
                    "Failed to launch app {} for URI {}: {}",
                    self.app.id().unwrap(),
                    item.target.description(),
                    error
                ))
            })
        } else {
            error!(
                "Item with ID {} not found for {}",
                id,
                self.app.id().unwrap()
            );
            Err(zbus::fdo::Error::Failed(format!("Result {} not found", id)))
        };
        trace!(
            "ActivateResult({}, {:?}, {}) -> {:?}",
            id,
            terms,
            timestamp,
            result
        );
        result
    }

    /// Launch a search within the App.
    ///
    /// This function is called when the user clicks on the provider icon to display more search results in the application.
    /// The arguments are the current search terms and a timestamp.
    ///
    /// Currently it simply launches the app without any arguments.
    fn launch_search(&self, terms: Vec<String>, timestamp: u32) -> zbus::fdo::Result<()> {
        trace!("Enter LaunchSearch({:?}, {:?})", terms, timestamp);
        info!("Launching app {} directly", self.app.id().unwrap().as_str());
        let result = self
            .app
            .launch(&[], Some(&self.launch_context))
            .map_err(|error| {
                error!(
                    "Failed to launch app {}: {:#}",
                    self.app.id().unwrap(),
                    error
                );
                zbus::fdo::Error::SpawnFailed(format!(
                    "Failed to launch app {}: {}",
                    self.app.id().unwrap(),
                    error
                ))
            });
        trace!(
            "Enter LaunchSearch({:?}, {:?}) -> {:?}",
            terms,
            timestamp,
            result
        );
        result
    }
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
