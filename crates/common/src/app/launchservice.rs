// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! A service to launch apps on a given Glib main context.

use std::convert::TryFrom;
use std::fmt::{Display, Formatter};
use std::sync::Arc;

use gio::prelude::*;
// use glib::clone;
use libc::pid_t;
use log::{debug, error, info, trace, warn};
use zbus::zvariant::OwnedObjectPath;

use crate::glib::VariantDict;
use crate::systemd::*;

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
    scope_settings: &SystemdScopeSettings,
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

fn get_pid(value: &glib::Variant) -> Option<pid_t> {
    value
        .get::<VariantDict>()
        .and_then(|data| data.lookup_value("pid", None))
        .and_then(|value| value.get::<pid_t>())
}

/// The desktop ID of an app.
#[derive(Debug, PartialEq, Clone)]
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

/// Request to launch an app, with an URI to launch with.
#[derive(Debug)]
struct AppLaunchRequest {
    app: AppId,
    uri: Option<String>,
    response: futures_channel::oneshot::Sender<Result<(), glib::Error>>,
}

/// A service which launches apps on a glib main context.
#[derive(Debug)]
pub struct AppLaunchService {
    source: Option<glib::SourceId>,
    send: glib::Sender<AppLaunchRequest>,
}

async fn launch_app(
    context: &gio::AppLaunchContext,
    app_id: &AppId,
    uri: Option<&str>,
) -> Result<(), glib::Error> {
    let app = gio::DesktopAppInfo::try_from(app_id)?;
    match uri {
        None => app.launch_uris_async_future(&[], Some(context)),
        Some(uri) => app.launch_uris_async_future(&[uri], Some(context)),
    }
    .await
}

fn handle_launched(
    main_context: glib::MainContext,
    connection: zbus::Connection,
    scope_settings: Arc<SystemdScopeSettings>,
    app: &gio::AppInfo,
    platform_data: &glib::Variant,
) {
    match get_pid(platform_data) {
        None => {
            warn!(
                "Failed to get PID of launched application from {:?}",
                platform_data
            );
        }
        Some(pid) => {
            info!("App {} launched with PID {}", app.id().unwrap(), pid);
            let id = app.id().unwrap().to_string();
            let description = app.description().map_or_else(
                || format!("app started by {}", scope_settings.started_by),
                |value| format!("{} started by {}", value, scope_settings.started_by),
            );
            main_context.spawn(async move {
                let result = move_launched_process_to_scope(
                    &connection,
                    &id,
                    &description,
                    &scope_settings,
                    pid,
                )
                    .await;
                match result {
                    Err(err) => {
                        error!("Failed to move running process {} of app {} into new systemd scope: {}", pid, id, err);
                    },
                    Ok((name, path)) => {
                        info!("Moved running process {} of app {} into new systemd scope {} at {}",pid, id, &name, path.into_inner());
                    },
                };
            })
        }
    }
}

fn handle_launch(
    main_context: glib::MainContext,
    launch_context: gio::AppLaunchContext,
    request: AppLaunchRequest,
) {
    main_context.spawn_local(async move {
        // We don't care if the receiver already dropped their side of the channel
        let _ = request
            .response
            .send(launch_app(&launch_context, &request.app, request.uri.as_deref()).await);
    });
}

impl AppLaunchService {
    /// Create a new launch service.
    pub fn new(
        main_context: &glib::MainContext,
        connection: zbus::Connection,
        scope_settings: SystemdScopeSettings,
    ) -> Self {
        let scope_settings_arc = Arc::new(scope_settings);
        let (send, recv) = glib::MainContext::channel(glib::Priority::default());

        let launch_context = gio::AppLaunchContext::new();
        launch_context.connect_launched(
            glib::clone!(@strong main_context => move |_, app, platform_data| {
                trace!(
                    "App {} launched with platform_data: {:?}",
                    app.id().unwrap(),
                    platform_data
                );
                handle_launched(
                    main_context.clone(),
                    connection.clone(),
                    scope_settings_arc.clone(),
                    app,
                    platform_data,
                )
            }),
        );

        let source = Some(recv.attach(
            Some(main_context),
            glib::clone!(@strong main_context => move |request: AppLaunchRequest| {
                handle_launch(main_context.clone(), launch_context.clone(), request);
                glib::Continue(true)
            }),
        ));
        AppLaunchService { source, send }
    }

    /// Create a new client for this launch service.
    pub fn client(&self) -> AppLaunchClient {
        AppLaunchClient {
            send: self.send.clone(),
        }
    }
}

impl Drop for AppLaunchService {
    fn drop(&mut self) {
        if let Some(source_id) = self.source.take() {
            glib::source_remove(source_id)
        }
    }
}

/// A client for a launch service.
#[derive(Debug, Clone)]
pub struct AppLaunchClient {
    send: glib::Sender<AppLaunchRequest>,
}

impl AppLaunchClient {
    async fn launch(&self, app: AppId, uri: Option<String>) -> Result<(), glib::Error> {
        let (response_tx, response_rx) = futures_channel::oneshot::channel();
        self.send
            .send(AppLaunchRequest {
                app,
                uri,
                response: response_tx,
            })
            .map_err(|err| {
                glib::Error::new(
                    glib::FileError::Failed,
                    &format!("Failed to launch app: {}", err),
                )
            })?;
        // If the sender was dropped
        response_rx.await.unwrap_or_else(|_| {
            Err(glib::Error::new(
                glib::FileError::Failed,
                "App launch service did not reply",
            ))
        })
    }

    /// Launch an app without arguments
    pub async fn launch_app(&self, app: AppId) -> Result<(), glib::Error> {
        self.launch(app, None).await
    }

    /// Launch the app with an URI.
    pub async fn launch_uri(&self, app: AppId, uri: String) -> Result<(), glib::Error> {
        self.launch(app, Some(uri)).await
    }
}
