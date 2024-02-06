// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! The search provider service for recent projects in Jetbrains products.

use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use elementtree::Element;
use gio::prelude::*;
use glib::{Variant, VariantDict};
use indexmap::IndexMap;
use tracing::{event, instrument, span, Level, Span};
use tracing_futures::Instrument;
use zbus::zvariant::{OwnedObjectPath, Value};
use zbus::{dbus_interface, zvariant};

use crate::config::ConfigLocation;
use crate::systemd;
use crate::systemd::Systemd1ManagerProxy;

/// The desktop ID of an app.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct AppId(String);

impl Display for AppId {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl TryFrom<&AppId> for gio::DesktopAppInfo {
    type Error = glib::Error;

    fn try_from(value: &AppId) -> Result<Self, Self::Error> {
        gio::DesktopAppInfo::new(&value.0).ok_or_else(|| {
            glib::Error::new(
                glib::FileError::Noent,
                &format!("App {} not found", value.0),
            )
        })
    }
}

impl From<String> for AppId {
    fn from(v: String) -> Self {
        Self(v)
    }
}

impl From<&str> for AppId {
    fn from(v: &str) -> Self {
        v.to_string().into()
    }
}

impl From<&gio::DesktopAppInfo> for AppId {
    fn from(app: &gio::DesktopAppInfo) -> Self {
        AppId(app.id().unwrap().to_string())
    }
}

/// An app that can be launched.
#[derive(Debug)]
pub struct App {
    /// The ID of this app
    id: AppId,
    /// The icon to use for this app
    icon: String,
}

impl App {
    /// The ID of this app.
    pub fn id(&self) -> &AppId {
        &self.id
    }

    /// The icon of this app.
    pub fn icon(&self) -> &str {
        &self.icon
    }
}

impl From<gio::DesktopAppInfo> for App {
    fn from(app: gio::DesktopAppInfo) -> Self {
        Self {
            id: (&app).into(),
            icon: IconExt::to_string(&app.icon().unwrap())
                .unwrap()
                .to_string(),
        }
    }
}

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
    let contents = std::fs::read_to_string(&name_file)
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

/// A recent project from a Jetbrains IDE.
///
/// Note that rider calls these solutions per dotnet lingo.
#[derive(Debug, PartialEq, Eq)]
pub struct JetbrainsRecentProject {
    /// The human readable project name.
    ///
    /// This is the name explicitly assigned by the user (if they did rename the project) or the
    /// last component of the project directory.
    name: String,

    /// The project directory.
    ///
    /// We deliberately use String here instead of `PathBuf`, since we never really operate on this
    /// as a path, but a `PathBuf` would loose us easy access to the string API for matching.
    directory: String,
}

#[instrument(fields(app_id = %app_id))]
fn read_recent_projects(
    config: &ConfigLocation<'_>,
    app_id: &AppId,
) -> Result<IndexMap<String, JetbrainsRecentProject>> {
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
            let mut recent_projects = IndexMap::new();
            for path in parse_recent_jetbrains_projects(home_s, &mut source)? {
                if let Some(name) = get_project_name(&path) {
                    event!(Level::TRACE, %app_id, "Found project {} at {}", name, path);
                    let id = format!("jetbrains-recent-project-{app_id}-{path}");
                    recent_projects.insert(
                        id,
                        JetbrainsRecentProject {
                            name,
                            directory: path.to_string(),
                        },
                    );
                } else {
                    event!(Level::TRACE, %app_id, "Skipping {}, failed to determine project name", path);
                }
            }
            event!(Level::INFO, %app_id, "Found {} recent project(s) for app {}", recent_projects.len(), app_id);
            Ok(recent_projects)
        }
        Err(error) => {
            event!(Level::DEBUG, %error, "No recent project available: {:#}", error);
            Ok(IndexMap::new())
        }
    }
}

fn get_pid(platform_data: &Variant) -> Option<i32> {
    match platform_data.get::<VariantDict>() {
        None => {
            event!(
                Level::ERROR,
                "platform_data not a dictionary, but {:?}",
                platform_data
            );
            None
        }
        // The type of the pid property doesn't seem to be documented anywhere, but variant type
        // errors indicate that the type is "i", i.e.gint32.
        //
        // See https://docs.gtk.org/glib/gvariant-format-strings.html#numeric-types
        Some(data) => match data.lookup::<i32>("pid") {
            Err(type_error) => {
                event!(
                    Level::ERROR,
                    "platform_data.pid had type {:?}, but expected {:?}",
                    type_error.actual,
                    type_error.expected
                );
                None
            }
            Ok(None) => {
                event!(
                    Level::WARN,
                    "pid missing in platform_data {:?}",
                    platform_data
                );
                None
            }
            Ok(Some(pid)) => Some(pid),
        },
    }
}

#[instrument(skip(connection))]
async fn move_to_scope(
    connection: &zbus::Connection,
    app_name: &str,
    pid: u32,
) -> Result<(String, OwnedObjectPath), zbus::Error> {
    let manager = Systemd1ManagerProxy::new(connection).await?;
    // See https://gitlab.gnome.org/jf/start-transient-unit/-/blob/117c6f32c8dc0d1f28686408f698632aa71880bc/rust/src/main.rs#L94
    // for inspiration.
    // See https://www.freedesktop.org/wiki/Software/systemd/ControlGroupInterface/ for background.
    let props = &[
        // I haven't found any documentation for the type of the PIDs property directly, but elsewhere
        // in its DBus interface system always used u32 for PIDs.
        ("PIDs", Value::Array(vec![pid].into())),
        // libgnome passes this property too, see
        // https://gitlab.gnome.org/GNOME/gnome-desktop/-/blob/106a729c3f98b8ee56823a0a49fa8504f78dd355/libgnome-desktop/gnome-systemd.c#L100
        //
        // I'm not entirely sure how it's relevant but it seems a good idea to do what Gnome does.
        ("CollectMode", Value::Str("inactive-or-failed".into())),
    ];
    let name = format!(
        "app-{}-{}-{}.scope",
        env!("CARGO_BIN_NAME"),
        systemd::escape_name(app_name.trim_end_matches(".desktop")),
        pid
    );
    event!(
        Level::DEBUG,
        "Creating new scope {name} for PID {pid} of {app_name} with {props:?}"
    );
    let scope_object_path = manager
        .start_transient_unit(&name, "fail", props, &[])
        .await?;
    Ok((name, scope_object_path))
}

/**
 * Create a launch context.
 *
 * This context moves all launched applications to their own system scope.
 */
fn create_launch_context(connection: zbus::Connection) -> gio::AppLaunchContext {
    let context = gio::AppLaunchContext::new();
    context.connect_launched(move |_, app, platform_data| {
        let app_id = app.id().unwrap().to_string();
        let _guard = span!(Level::INFO, "launched", %app_id, %platform_data).entered();
        event!(
            Level::TRACE,
            "App {} launched with platform_data: {:?}",
            app_id,
            platform_data
        );
        if let Some(pid) = get_pid(platform_data) {
            event!(Level::INFO, "App {} launched with PID {pid}", app.id().unwrap());
            let app_name = app.id().unwrap().to_string();
            let connection_inner = connection.clone();
            glib::MainContext::ref_thread_default().spawn(
                async move {
                    match move_to_scope(&connection_inner, &app_name, pid as u32).await {
                        Err(err) => {
                            event!(Level::ERROR, "Failed to move running process {pid} of app {app_name} into new systemd scope: {err}");
                        },
                        Ok((name, path)) => {
                            event!(Level::INFO, "Moved running process {pid} of app {app_name} into new systemd scope {name} at {}", path.into_inner());
                        },
                    }
                }.in_current_span(),
            );
        }
    });
    context
}

/// Launch the given app, optionally passing a given URI.
///
/// Move the launched app to a dedicated systemd scope for resource control, and return the result
/// of launching the app.
#[instrument(skip(connection))]
async fn launch_app_in_new_scope(
    connection: zbus::Connection,
    app_id: AppId,
    uri: Option<String>,
) -> zbus::fdo::Result<()> {
    let context = create_launch_context(connection);
    let app = gio::DesktopAppInfo::try_from(&app_id).map_err(|error| {
        event!(
            Level::ERROR,
            %error,
            "Failed to find app {app_id}: {error:#}"
        );
        zbus::fdo::Error::Failed(format!("Failed to find app {app_id}: {error}"))
    })?;
    match uri {
        None => app.launch_uris_future(&[], Some(&context)),
        Some(ref uri) => app.launch_uris_future(&[uri], Some(&context)),
    }
    .await
    .map_err(|error| {
        event!(
            Level::ERROR,
            %error,
            "Failed to launch app {app_id} with {uri:?}: {error:#}",
        );
        zbus::fdo::Error::Failed(format!(
            "Failed to launch app {app_id} with {uri:?}: {error}"
        ))
    })
}

/// A search provider for recent Jetbrains products.
#[derive(Debug)]
pub struct JetbrainsProductSearchProvider {
    app: App,
    recent_projects: IndexMap<String, JetbrainsRecentProject>,
    config: &'static ConfigLocation<'static>,
}

impl JetbrainsProductSearchProvider {
    /// Create a new search provider for a jetbrains product.
    ///
    /// `app` describes the underlying app to launch projects with, and `config` describes
    /// where this Jetbrains product has its configuration.
    pub fn new(app: App, config: &'static ConfigLocation<'static>) -> Self {
        Self {
            app,
            config,
            recent_projects: IndexMap::new(),
        }
    }

    /// Get the underyling app for this Jetbrains product.
    pub fn app(&self) -> &App {
        &self.app
    }

    /// Reload all recent projects provided by this search provider.
    pub fn reload_recent_projects(&mut self) -> Result<()> {
        self.recent_projects = read_recent_projects(self.config, self.app.id())?;
        Ok(())
    }

    #[instrument(skip(self, connection), fields(app_id = %self.app.id()))]
    async fn launch_app_on_default_main_context(
        &self,
        connection: zbus::Connection,
        uri: Option<String>,
    ) -> zbus::fdo::Result<()> {
        let app_id = self.app.id().clone();
        let span = Span::current();
        glib::MainContext::default()
            .spawn_from_within(move || {
                launch_app_in_new_scope(connection, app_id, uri.clone()).instrument(span)
            })
            .await
            .map_err(|error| {
                event!(
                    Level::ERROR,
                    %error,
                    "Join from main loop failed: {error:#}",
                );
                zbus::fdo::Error::Failed(format!("Join from main loop failed: {error:#}",))
            })?
    }
}

/// Calculate how well `recent_projects` matches all of the given `terms`.
///
/// If all terms match the name of the `recent_projects`, the project receives a base score of 10.
/// If all terms match the directory of the `recent_projects`, the project gets scored for each
/// term according to how far right the term appears in the directory, under the assumption that
/// the right most part of a directory path is the most specific.
///
/// All matches are done on the lowercase text, i.e. case insensitve.
fn score_recent_project(recent_project: &JetbrainsRecentProject, terms: &[&str]) -> f64 {
    let name = recent_project.name.to_lowercase();
    let directory = recent_project.directory.to_lowercase();
    terms
        .iter()
        .try_fold(0.0, |score, term| {
            directory
                .rfind(&term.to_lowercase())
                // We add 1 to avoid returning zero if the term matches right at the beginning.
                .map(|index| score + ((index + 1) as f64 / recent_project.directory.len() as f64))
        })
        .unwrap_or(0.0)
        + if terms.iter().all(|term| name.contains(&term.to_lowercase())) {
            10.0
        } else {
            0.0
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
    fn get_initial_result_set(&self, terms: Vec<&str>) -> Vec<&str> {
        event!(Level::DEBUG, "Searching for {:?}", terms);
        let mut scored_ids = self
            .recent_projects
            .iter()
            .filter_map(|(id, item)| {
                let score = score_recent_project(item, &terms);
                if 0.0 < score {
                    Some((id.as_ref(), score))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        scored_ids.sort_by_key(|(_, score)| -((score * 1000.0) as i64));
        let ids = scored_ids.into_iter().map(|(id, _)| id).collect();
        event!(Level::DEBUG, "Found ids {:?}", ids);
        ids
    }

    /// Refine an ongoing search.
    ///
    /// This function is called to refine the initial search results when the user types more characters in the search entry.
    /// It gets the previous search results and the current search terms as arguments, and should return an array of result IDs,
    /// just like GetInitialResultSet.
    #[instrument(skip(self), fields(app_id = %self.app.id()))]
    fn get_subsearch_result_set(&self, previous_results: Vec<&str>, terms: Vec<&str>) -> Vec<&str> {
        event!(
            Level::DEBUG,
            "Searching for {:?} in {:?}",
            terms,
            previous_results
        );
        // For simplicity just run the overall search again, and filter out everything not already matched.
        let ids = self
            .get_initial_result_set(terms)
            .into_iter()
            .filter(|id| previous_results.contains(id))
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
            if let Some(item) = self.recent_projects.get(&item_id) {
                event!(Level::DEBUG, %item_id, "Compiling meta info for {}", item_id);
                let mut meta: HashMap<String, zvariant::Value> = HashMap::new();
                meta.insert("id".to_string(), item_id.clone().into());
                meta.insert("name".to_string(), item.name.clone().into());
                event!(Level::DEBUG, %item_id, "Using icon {}", self.app.icon());
                meta.insert("gicon".to_string(), self.app.icon().to_string().into());
                meta.insert("description".to_string(), item.directory.clone().into());
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
    #[instrument(skip(self, connection), fields(app_id = %self.app.id()))]
    async fn activate_result(
        &mut self,
        #[zbus(connection)] connection: &zbus::Connection,
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
        if let Some(item) = self.recent_projects.get(item_id) {
            event!(Level::INFO, item_id, "Launching recent item {:?}", item);
            self.launch_app_on_default_main_context(
                connection.clone(),
                Some(item.directory.clone()),
            )
            .await
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
    #[instrument(skip(self, connection), fields(app_id = %self.app.id()))]
    async fn launch_search(
        &self,
        #[zbus(connection)] connection: &zbus::Connection,
        _terms: Vec<String>,
        _timestamp: u32,
    ) -> zbus::fdo::Result<()> {
        event!(Level::DEBUG, "Launching app directly");
        self.launch_app_on_default_main_context(connection.clone(), None)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use similar_asserts::assert_eq;

    #[test]
    fn read_recent_projects() {
        let data: &[u8] = include_bytes!("tests/recentProjects.xml");
        let home = glib::home_dir();
        let recent_projects =
            parse_recent_jetbrains_projects(home.to_str().unwrap(), data).unwrap();

        assert_eq!(
            recent_projects,
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
        let recent_projects =
            parse_recent_jetbrains_projects(home.to_str().unwrap(), data).unwrap();

        assert_eq!(
            recent_projects,
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
