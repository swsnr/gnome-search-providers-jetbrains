// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! A service to launch apps on a given Glib main context.

use crate::app::AppLaunchTarget;
use gio::prelude::*;
use log::error;
use std::convert::TryFrom;
use std::fmt::{Display, Formatter};

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

/// Request to launch an app, with a file or URI to launch with.
#[derive(Debug)]
pub struct AppLaunchRequest {
    /// The ID of the app to launch
    app: AppId,
    target: Option<AppLaunchTarget>,
}

impl AppLaunchRequest {
    /// Launch an app without arguments.
    pub fn without_args(app: AppId) -> Self {
        Self { app, target: None }
    }

    /// Launch an app with the given launch target.
    pub fn with_arg(app: AppId, target: AppLaunchTarget) -> Self {
        Self {
            app,
            target: Some(target),
        }
    }
}

/// A service which launches apps on a glib main context.
#[derive(Debug)]
pub struct AppLaunchService {
    source: glib::SourceId,
    send: glib::Sender<AppLaunchRequest>,
}

fn launch_app(
    context: &gio::AppLaunchContext,
    request: &AppLaunchRequest,
) -> Result<(), glib::Error> {
    let app = gio::DesktopAppInfo::try_from(&request.app)?;
    match &request.target {
        None => app.launch(&[], Some(context)),
        Some(AppLaunchTarget::File(path)) => {
            app.launch::<gio::AppLaunchContext>(&[gio::File::for_path(path)], Some(context))
        }
        Some(AppLaunchTarget::Uri(uri)) => app.launch_uris(&[uri], Some(context)),
    }
}

impl AppLaunchService {
    /// Create a new launch service.
    pub fn new(main_context: &glib::MainContext, launch_context: gio::AppLaunchContext) -> Self {
        let (send, recv) = glib::MainContext::channel(glib::Priority::default());
        let source = recv.attach(Some(main_context), move |request: AppLaunchRequest| {
            if let Err(error) = launch_app(&launch_context, &request) {
                error!(
                    "Failed to launch app {} for target {:?}: {}",
                    request.app, request.target, error,
                );
            }
            glib::Continue(true)
        });
        AppLaunchService { source, send }
    }

    /// Create a new client for this launch service.
    pub fn client(&self) -> AppLaunchClient {
        AppLaunchClient {
            send: self.send.clone(),
        }
    }
}

/// A client for a launch service.
#[derive(Debug, Clone)]
pub struct AppLaunchClient {
    send: glib::Sender<AppLaunchRequest>,
}

impl AppLaunchClient {
    /// Submit a request to launch an app.
    pub fn launch(&self, request: AppLaunchRequest) -> Result<(), glib::Error> {
        self.send.send(request).map_err(|err| {
            glib::Error::new(
                glib::FileError::Failed,
                &format!("Failed to launch app: {}", err),
            )
        })
    }
}
