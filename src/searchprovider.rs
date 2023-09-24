// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! The search provider service for recent projects in Jetbrains products.

use anyhow::{Context, Result};
use elementtree::Element;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use tracing::{event, instrument, Level};
use zbus::{dbus_interface, zvariant};

use gnome_search_provider_common::app::{App, AppId, AppLaunchClient, AppLaunchItem};
use gnome_search_provider_common::glib;
use gnome_search_provider_common::matching::{find_matching_items, IndexMap};

use crate::config::ConfigLocation;

/// Read paths of all recent projects from the given `reader`.
fn parse_recent_jetbrains_projects<R: Read>(home: &str, reader: R) -> Result<Vec<String>> {
    let element = Element::from_reader(reader)?;
    event!(Level::TRACE, "Finding projects in {:?}", element);

    let projects = element
        .find_all("component")
        .find(|e| {
            e.get_attr("name") == Some("RecentProjectsManager")
                || e.get_attr("name") == Some("RiderRecentProjectsManager")
        })
        .and_then(|comp| {
            comp.find_all("option")
                .find(|e| e.get_attr("name") == Some("additionalInfo"))
        })
        .and_then(|opt| opt.find("map"))
        .map(|map| {
            map.find_all("entry")
                .filter_map(|entry| entry.get_attr("key"))
                .map(|key| key.replace("$USER_HOME$", home))
                .collect()
        })
        .unwrap_or_default();

    event!(
        Level::TRACE,
        "Parsed projects {:?} from {:?}",
        projects,
        element
    );

    Ok(projects)
}

/// Try to read the name of a Jetbrains project from the `name` file of the given project directory.
///
/// Look for a `name` file in the `.idea` sub-directory and return the contents of this file.
fn read_name_from_file<P: AsRef<Path>>(path: P) -> Result<String> {
    let name_file = path.as_ref().join(".idea").join(".name");
    event!(
        Level::TRACE,
        "Trying to read name from {}",
        name_file.display()
    );
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read project name from {}", name_file.display()))?;
    Ok(contents.trim().to_string())
}

/// Get the name of the Jetbrains product at the given path.
///
/// Look for a `name` file in the `.idea` sub-directory; if that file does not exist
/// or cannot be read take the file name of `path`, and ultimately return `None` if
/// the name cannot be determined.
fn get_project_name<P: AsRef<Path>>(path: P) -> Option<String> {
    match read_name_from_file(path.as_ref()) {
        Ok(name) => Some(name),
        Err(error) => {
            event!(
                Level::DEBUG,
                "Failed to read project name from file {:#}; falling back to file name of {}",
                error,
                path.as_ref().display()
            );
            path.as_ref()
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
        }
    }
}

#[instrument(fields(app_id = %app_id))]
fn read_recent_items(
    config: &ConfigLocation<'_>,
    app_id: &AppId,
) -> Result<IndexMap<String, AppLaunchItem>> {
    event!(Level::INFO, %app_id, "Reading recents projects of {}", app_id);
    match config
        .find_latest_recent_projects_file(&glib::user_config_dir())
        .and_then(|projects_file| {
            File::open(&projects_file).with_context(|| {
                format!(
                    "Failed to open recent projects file at {}",
                    projects_file.display()
                )
            })
        }) {
        Ok(mut source) => {
            let home = glib::home_dir();
            let home_s = home
                .to_str()
                .with_context(|| "Failed to convert home directory path to UTF-8 string")?;
            let mut items = IndexMap::new();
            for path in parse_recent_jetbrains_projects(home_s, &mut source)? {
                if let Some(name) = get_project_name(&path) {
                    event!(Level::TRACE, %app_id, "Found project {} at {}", name, path);
                    let id = format!("jetbrains-recent-project-{app_id}-{path}");
                    items.insert(
                        id,
                        AppLaunchItem {
                            name,
                            uri: path.to_string(),
                        },
                    );
                } else {
                    event!(Level::TRACE, %app_id, "Skipping {}, failed to determine project name", path);
                }
            }
            event!(Level::INFO, %app_id, "Found {} recent project(s) for app {}", items.len(), app_id);
            Ok(items)
        }
        Err(error) => {
            event!(Level::DEBUG, %error, "No recent items available: {:#}", error);
            Ok(IndexMap::new())
        }
    }
}

#[derive(Debug)]
pub struct JetbrainsProductSearchProvider {
    app: App,
    items: IndexMap<String, AppLaunchItem>,
    launcher: AppLaunchClient,
    config: &'static ConfigLocation<'static>,
}

impl JetbrainsProductSearchProvider {
    /// Create a new search provider for a jetbrains product.
    ///
    /// `app` describes the underlying app to launch items with, and `launcher` providers a service
    /// to launch apps from the Glib main loop.  `config` describes where this Jetbrains product has
    /// its configuration.
    ///
    /// `pool` is a thread pool to run IO on.
    pub fn new(
        app: App,
        launcher: AppLaunchClient,
        config: &'static ConfigLocation<'static>,
    ) -> Self {
        Self {
            app,
            launcher,
            config,
            items: IndexMap::new(),
        }
    }

    /// Get the underyling app for this Jetbrains product.
    pub fn app(&self) -> &App {
        &self.app
    }

    /// Reload all recent items provided by this search provider.
    pub fn reload_items(&mut self) -> Result<()> {
        self.items = read_recent_items(self.config, self.app.id())?;
        Ok(())
    }
}

/// The DBus interface of the search provider.
///
/// See <https://developer.gnome.org/SearchProvider/> for information.
#[dbus_interface(name = "org.gnome.Shell.SearchProvider2")]
impl JetbrainsProductSearchProvider {
    /// Starts a search.
    ///
    /// This function is called when a new search is started. It gets an array of search terms as arguments,
    /// and should return an array of result IDs. gnome-shell will call GetResultMetas for (some) of these result
    /// IDs to get details about the result that can be be displayed in the result list.
    #[instrument(skip(self), fields(app_id = %self.app.id()))]
    fn get_initial_result_set(&self, terms: Vec<&str>) -> Vec<String> {
        event!(Level::DEBUG, "Searching for {:?}", terms);
        let ids = find_matching_items(self.items.iter(), terms.as_slice())
            .into_iter()
            .map(String::to_owned)
            .collect();
        event!(Level::DEBUG, "Found ids {:?}", ids);
        ids
    }

    /// Refine an ongoing search.
    ///
    /// This function is called to refine the initial search results when the user types more characters in the search entry.
    /// It gets the previous search results and the current search terms as arguments, and should return an array of result IDs,
    /// just like GetInitialResultSet.
    #[instrument(skip(self), fields(app_id = %self.app.id()))]
    fn get_subsearch_result_set(
        &self,
        previous_results: Vec<&str>,
        terms: Vec<&str>,
    ) -> Vec<String> {
        event!(
            Level::DEBUG,
            "Searching for {:?} in {:?}",
            terms,
            previous_results
        );
        let candidates = previous_results
            .iter()
            .filter_map(|&id| self.items.get(id).map(|p| (id, p)));

        let ids = find_matching_items(candidates, terms.as_slice())
            .into_iter()
            .map(|s| s.to_owned())
            .collect();
        event!(Level::DEBUG, "Found ids {:?}", ids);
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
    //  - "gicon": a textual representation of a GIcon (see g_icon_to_string()), or alternatively,
    //  - "icon-data": a tuple of type (iiibiiay) describing a pixbuf with width, height, rowstride, has-alpha, bits-per-sample, and image data
    //  - "description": an optional short description (1-2 lines)
    #[instrument(skip(self), fields(app_id = %self.app.id()))]
    fn get_result_metas(
        &self,
        results: Vec<String>,
    ) -> zbus::fdo::Result<Vec<HashMap<String, zvariant::Value<'_>>>> {
        event!(Level::DEBUG, "Getting meta info for {:?}", results);
        let mut metas = Vec::with_capacity(results.len());
        for item_id in results {
            if let Some(item) = self.items.get(&item_id) {
                event!(Level::DEBUG, %item_id, "Compiling meta info for {}", item_id);
                let mut meta: HashMap<String, zvariant::Value> = HashMap::new();
                meta.insert("id".to_string(), item_id.clone().into());
                meta.insert("name".to_string(), item.name.clone().into());
                event!(Level::DEBUG, %item_id, "Using icon {}", self.app.icon());
                meta.insert("gicon".to_string(), self.app.icon().to_string().into());
                meta.insert("description".to_string(), item.uri.clone().into());
                metas.push(meta);
            }
        }
        event!(Level::DEBUG, "Return meta info {:?}", &metas);
        Ok(metas)
    }

    /// Activate an individual result.
    ///
    /// This function is called when the user clicks on an individual result to open it in the application.
    /// The arguments are the result ID, the current search terms and a timestamp.
    ///
    /// Launches the underlying app with the path to the selected item.
    #[instrument(skip(self), fields(app_id = %self.app.id()))]
    async fn activate_result(
        &mut self,
        item_id: &str,
        terms: Vec<&str>,
        timestamp: u32,
    ) -> zbus::fdo::Result<()> {
        event!(
            Level::DEBUG,
            item_id,
            "Activating result {} for {:?} at {}",
            item_id,
            terms,
            timestamp
        );
        if let Some(item) = self.items.get(item_id) {
            event!(Level::INFO, item_id, "Launching recent item {:?}", item);
            self.launcher
                .launch_uri(self.app.id().clone(), item.uri.clone())
                .await
                .map_err(|error| {
                    event!(
                        Level::ERROR,
                        %error,
                        "Failed to launch app {} for {:?}: {:#}",
                        self.app.id(),
                        item.uri,
                        error
                    );
                    zbus::fdo::Error::Failed(format!(
                        "Failed to launch app {} for {}: {}",
                        self.app.id(),
                        item.uri,
                        error
                    ))
                })
        } else {
            event!(Level::ERROR, item_id, "Item not found");
            Err(zbus::fdo::Error::Failed(format!(
                "Result {item_id} not found"
            )))
        }
    }

    /// Launch a search within the App.
    ///
    /// This function is called when the user clicks on the provider icon to display more search results in the application.
    /// The arguments are the current search terms and a timestamp.
    ///
    /// Currently it simply launches the app without any arguments.
    #[instrument(skip(self), fields(app_id = %self.app.id()))]
    async fn launch_search(&self, _terms: Vec<String>, _timestamp: u32) -> zbus::fdo::Result<()> {
        event!(Level::DEBUG, "Launching app directly");
        self.launcher
            .launch_app(self.app.id().clone())
            .await
            .map_err(|error| {
                event!(Level::ERROR, %error, "Failed to launch app {}: {:#}", self.app.id(), error);
                zbus::fdo::Error::Failed(format!(
                    "Failed to launch app {}: {}",
                    self.app.id(),
                    error
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gnome_search_provider_common::glib;
    use similar_asserts::assert_eq;

    #[test]
    fn read_recent_projects() {
        let data: &[u8] = include_bytes!("tests/recentProjects.xml");
        let home = glib::home_dir();
        let items = parse_recent_jetbrains_projects(home.to_str().unwrap(), data).unwrap();

        assert_eq!(
            items,
            vec![
                home.join("Code")
                    .join("gh")
                    .join("mdcat")
                    .to_string_lossy()
                    .to_string(),
                home.join("Code")
                    .join("gh")
                    .join("gnome-search-providers-jetbrains")
                    .to_string_lossy()
                    .to_string()
            ]
        )
    }

    #[test]
    fn read_recent_solutions() {
        let data: &[u8] = include_bytes!("tests/recentSolutions.xml");
        let home = glib::home_dir();
        let items = parse_recent_jetbrains_projects(home.to_str().unwrap(), data).unwrap();

        assert_eq!(
            items,
            vec![
                home.join("Code")
                    .join("gh")
                    .join("mdcat")
                    .to_string_lossy()
                    .to_string(),
                home.join("Code")
                    .join("gh")
                    .join("gnome-search-providers-jetbrains")
                    .to_string_lossy()
                    .to_string()
            ]
        )
    }
}
