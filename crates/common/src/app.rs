// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Items to be launched by an app.

use std::collections::HashMap;

use gio::{AppInfoExt, AppLaunchContextExt, IconExt};
use glib::VariantDict;
use indexmap::IndexMap;
use libc::pid_t;
use log::{debug, error, info, trace, warn};
use zbus::dbus_interface;
use zbus::export::zvariant;

use crate::matching::*;
use crate::systemd::{ScopeProperties, Systemd1ManagerExt, Systemd1ManagerProxy};

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
        trace!("Score for name {} of {:?}: {:?}", &name, &self, name_score);
        let target_score = terms.iter().try_fold(0.0, |score, term| {
            target
                .rfind(&term.as_ref().to_lowercase())
                .ok_or(())
                .map(|index| score + 1.0 * (index as f64 / target.len() as f64))
        });
        trace!(
            "Score for target {} of {:?}: {:?}",
            &target,
            &self,
            target_score
        );
        name_score.unwrap_or_default() + target_score.unwrap_or_default()
    }
}

/// Settings for systemd scopes created by a search provider for launched apps.
#[derive(Debug)]
pub struct SystemdScopeSettings {
    /// The prefix for scope names.
    pub prefix: String,
    /// A string which identifies the app which started the scope.
    pub started_by: String,
    /// Optional documentation URLs for the scope.
    pub documentation: Vec<String>,
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
    pub fn new(
        app: gio::DesktopAppInfo,
        source: S,
        systemd: Systemd1ManagerProxy<'static>,
        scope_settings: SystemdScopeSettings,
    ) -> Self {
        let launch_context = gio::AppLaunchContext::new();
        launch_context.connect_launched(move |_, app, platform_data| {
            trace!("App {} launched with data: {:?}", app.get_id().unwrap(), platform_data);
            match platform_data
                .get::<VariantDict>()
                .and_then(|data| data.lookup_value("pid", None))
                .and_then(|value| value.get::<pid_t>())
            {
                None => warn!(
                    "Failed to get PID of launched application from {:?}",
                    platform_data,
                ),
                Some(pid) => {
                    info!("App {} launched with PID {}", app.get_id().unwrap(), pid);
                    // Gnome also strips the .desktop suffix from IDs, see
                    // https://gitlab.gnome.org/GNOME/gnome-desktop/-/blob/106a729c3f98b8ee56823a0a49fa8504f78dd355/libgnome-desktop/gnome-systemd.c#L227
                    let id = app.get_id().unwrap();
                    let description = app.get_description()
                        .map_or_else(|| format!("app started by {}", scope_settings.started_by), |value| format!("{} started by {}", value, scope_settings.started_by));
                    let properties = ScopeProperties {
                        prefix: &scope_settings.prefix,
                        name: id.trim_end_matches(".desktop"),
                        description: Some(description.as_str()),
                        documentation: scope_settings.documentation.iter().map(|v| v.as_str()).collect(),
                    };
                    trace!("start_app_scope({:?}, {})", &properties, pid);
                    match systemd.start_app_scope(properties, pid) {
                        Err(err) => error!("Failed to move running process {} of app {} into new systemd scope: {}", pid, app.get_id().unwrap(), err),
                        Ok((name, path)) => info!("Moved running process {} of app {} into new systemd scope {} at {}", pid, app.get_id().unwrap(), &name, path.into_inner()),
                    }
                }
            }
        });
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
    fn get_initial_result_set(&mut self, terms: Vec<String>) -> zbus::fdo::Result<Vec<String>> {
        debug!(
            "Searching for {:?} of {}",
            terms,
            self.app.get_id().unwrap()
        );
        self.items = self.source.find_recent_items().map_err(|error| {
            error!(
                "Failed to update recent items for {}: {:#}",
                self.app.get_id().unwrap(),
                error
            );
            zbus::fdo::Error::Failed(format!(
                "Failed to update recent items for {}: {:#}",
                self.app.get_id().unwrap(),
                error
            ))
        })?;

        let ids = find_matching_items(self.items.iter(), terms.as_slice())
            .into_iter()
            .map(String::to_owned)
            .collect();
        debug!("Found ids {:?} for {}", ids, self.app.get_id().unwrap());
        Ok(ids)
    }

    /// Refine an ongoing search.
    ///
    /// This function is called to refine the initial search results when the user types more characters in the search entry.
    /// It gets the previous search results and the current search terms as arguments, and should return an array of result IDs,
    /// just like GetInitialResulSet.
    fn get_subsearch_result_set(
        &self,
        previous_results: Vec<String>,
        terms: Vec<String>,
    ) -> Vec<String> {
        debug!(
            "Searching for {:?} in {:?} of {}",
            terms,
            previous_results,
            self.app.get_id().unwrap()
        );
        let candidates = previous_results
            .iter()
            .filter_map(|id| self.items.get(id).map(|p| (id, p)));

        let ids = find_matching_items(candidates, terms.as_slice())
            .into_iter()
            .map(String::to_owned)
            .collect();
        debug!("Found ids {:?} for {}", ids, self.app.get_id().unwrap());
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
        debug!("Getting meta info for {:?}", results);
        results
            .into_iter()
            .filter_map(|id| {
                self.items.get(&id).map(|item| {
                    debug!("Compiling meta info for {}", id);
                    let icon = IconExt::to_string(&self.app.get_icon().unwrap()).unwrap();
                    debug!("Using icon {} for id {}", icon, id);

                    let mut meta: HashMap<String, zvariant::Value> = HashMap::new();
                    meta.insert("id".to_string(), id.into());
                    meta.insert("name".to_string(), item.name.to_string().into());
                    meta.insert("gicon".to_string(), icon.to_string().into());
                    meta.insert(
                        "description".to_string(),
                        item.target.description().to_string().into(),
                    );
                    meta
                })
            })
            .collect()
    }

    /// Activate an individual result.
    ///
    /// This function is called when the user clicks on an individual result to open it in the application.
    /// The arguments are the result ID, the current search terms and a timestamp.
    ///
    /// Launches the underlying app with the path to the selected item.
    fn activate_result(
        &self,
        id: String,
        terms: Vec<String>,
        timestamp: u32,
    ) -> zbus::fdo::Result<()> {
        debug!("Activating result {} for {:?} at {}", id, terms, timestamp);
        if let Some(item) = self.items.get(&id) {
            info!("Launching recent item {:?}", item);
            match &item.target {
                AppLaunchTarget::File(path) => self.app.launch::<gio::AppLaunchContext>(
                    &[gio::File::new_for_path(path)],
                    Some(&self.launch_context),
                ),
                AppLaunchTarget::Uri(uri) => {
                    self.app.launch_uris(&[uri], Some(&self.launch_context))
                }
            }
            .map_err(|error| {
                error!(
                    "Failed to launch app {} for target {}: {}",
                    self.app.get_id().unwrap(),
                    item.target.description(),
                    error
                );
                zbus::fdo::Error::SpawnFailed(format!(
                    "Failed to launch app {} for URI {}: {}",
                    self.app.get_id().unwrap(),
                    item.target.description(),
                    error
                ))
            })
        } else {
            error!("Item with ID {} not found", id);
            Err(zbus::fdo::Error::Failed(format!("Result {} not found", id)))
        }
    }

    /// Launch a search within the App.
    ///
    /// This function is called when the user clicks on the provider icon to display more search results in the application.
    /// The arguments are the current search terms and a timestamp.
    ///
    /// Currently it simply launches the app without any arguments.
    fn launch_search(&self, terms: Vec<String>, timestamp: u32) -> zbus::fdo::Result<()> {
        debug!("Launching search for {:?} at {}", terms, timestamp);
        info!("Launching app {} directly", self.app.get_id().unwrap());
        self.app
            .launch(&[], Some(&self.launch_context))
            .map_err(|error| {
                error!(
                    "Failed to launch app {}: {:#}",
                    self.app.get_id().unwrap(),
                    error
                );
                zbus::fdo::Error::SpawnFailed(format!(
                    "Failed to launch app {}: {}",
                    self.app.get_id().unwrap(),
                    error
                ))
            })
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
