// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! The search provider service for recent projects in Jetbrains products.

use anyhow::{Context, Result};
use elementtree::Element;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use tracing::{event, instrument, Level, Span};
use tracing_futures::Instrument;

use gnome_search_provider_common::app::{AppId, AppItemSearchRequest, AppLaunchItem};
use gnome_search_provider_common::futures_channel::mpsc;
use gnome_search_provider_common::futures_util::StreamExt;
use gnome_search_provider_common::glib;
use gnome_search_provider_common::matching::IndexMap;

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

async fn get_items(
    app_id: AppId,
    config: &'static ConfigLocation<'static>,
    pool: &glib::ThreadPool,
) -> Result<IndexMap<String, AppLaunchItem>> {
    let id_inner = app_id.clone();
    let span_for_pool = Span::current();
    event!(Level::DEBUG, %app_id, "Spawning task on thread pool to read recent items");
    pool.push_future(move || span_for_pool.in_scope(|| read_recent_items(config, &id_inner)))
        .with_context(|| "Failed to run task on IO thread pool".to_string())?
        .await
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

/// Handle a single search provider request.
///
/// Handle `request` and return the new list of app items, if any.
#[instrument(skip(pool, items), fields(app_id=%app_id, request=%request.name()))]
async fn handle_search_provider_request(
    app_id: AppId,
    config: &'static ConfigLocation<'static>,
    pool: &glib::ThreadPool,
    items: Option<Arc<IndexMap<String, AppLaunchItem>>>,
    request: AppItemSearchRequest,
) -> Option<Arc<IndexMap<String, AppLaunchItem>>> {
    match request {
        AppItemSearchRequest::RefreshItems(span) => {
            span.in_scope(|| {
                event!(Level::DEBUG, %app_id, "Updating items");
            });
            let result = get_items(app_id.clone(), config, pool)
                .instrument(span.clone())
                .await;
            match result {
                Ok(items) => Some(Arc::new(items)),
                Err(error) => span.in_scope(|| {
                    event!(Level::ERROR, %app_id, %error, "Failed to get recent items: {:#}", error);
                    None
                })
            }
        }
        AppItemSearchRequest::GetItems(span, respond_to) => {
            let result = match &items {
                None => respond_to.send(Arc::new(IndexMap::new())),
                Some(items) => respond_to.send(items.clone()),
            };
            if result.is_err() {
                span.in_scope(|| {
                    event!(Level::ERROR, %app_id, "Cannot answer GetItems request, remote side closed prematurely");
                })
            }
            items
        }
    }
}

/// Serve search provider requests.
///
/// Loop over requests received from `rx`, and provide the search provider with appropriate
/// responses.
///
/// `pool` is used to spawn blocking IO.
pub async fn serve_search_provider(
    app_id: AppId,
    config: &'static ConfigLocation<'static>,
    pool: glib::ThreadPool,
    mut rx: mpsc::Receiver<AppItemSearchRequest>,
) {
    let mut items = None;
    loop {
        match rx.next().await {
            None => {
                event!(Level::DEBUG, %app_id, "No more requests from search provider, stopping");
                break;
            }
            Some(request) => {
                let span = request.span().clone();
                items =
                    handle_search_provider_request(app_id.clone(), config, &pool, items, request)
                        .instrument(span)
                        .await;
            }
        }
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
