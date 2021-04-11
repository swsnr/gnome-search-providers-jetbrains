#!/usr/bin/env python3
# Copyright Sebastian Wiesner <sebastian@swsnr.de>
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.


import re
import os
import logging
from functools import partial
from typing import Iterable, Optional, Any
from dataclasses import dataclass
from pathlib import Path
import xml.etree.ElementTree as etree
from operator import itemgetter

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from systemd.journal import JournalHandler
from gi.repository import GLib, Gio
from fuzzywuzzy import process, fuzz


log = logging.getLogger(__name__)


def xdg_config_home():
    """
    Find the XDG config directory.
    """
    return Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))


@dataclass
class RecentProject:
    """
    A recent project with a name and a path.
    """

    name: str
    path: Path


def read_recent_projects(filename: Path) -> Iterable[RecentProject]:
    """
    Read recent projects from the given file.
    """
    document = etree.parse(filename)
    paths = set(
        Path(el.attrib["value"].replace("$USER_HOME$", "~"))
        for el in document.findall('.//option[@name="recentPaths"]/list/option')
    )
    # Paths structure since IDEA 2020.3
    paths.update(
        Path(el.attrib["key"].replace("$USER_HOME$", "~")).expanduser()
        for el in document.findall(
            './/component[@name="RecentProjectsManager"]/option[@name="additionalInfo"]/map/entry'
        )
    )
    for path in paths:
        if path.exists():
            try:
                name = (path / ".idea" / "name").read_text(encoding="utf-8").strip()
            except FileNotFoundError:
                name = path.name
            yield RecentProject(name=name, path=path)


search_provider_method = partial(
    dbus.service.method, dbus_interface="org.gnome.Shell.SearchProvider2"
)


@dataclass
class ConfigLocation:
    vendor_dir: str
    config_glob: str

    def recent_projects(self) -> Optional[Path]:
        config_home = xdg_config_home()
        entries = (
            (entry, re.search(r"(\d{1,4}).(\d{1,2})", entry.name))
            for entry in (config_home / self.vendor_dir).glob(self.config_glob)
            if entry.is_dir()
        )
        versioned = (
            (entry, (int(match.group(1)), int(match.group(2))))
            for (entry, match) in entries
            if match
        )
        config_dir = max(versioned, key=lambda i: i[1], default=None)
        if config_dir:
            return config_dir[0] / "options" / "recentProjects.xml"
        else:
            return None


def find_matches(candidates: dict[str, RecentProject], terms: list[str]) -> list[str]:
    query = " ".join(terms)
    log.debug("Matching %s against %r", query, candidates)
    matches = process.extractBests(
        query,
        {id: project.name for id, project in candidates.items()},
        score_cutoff=50,
        limit=None,
        # Match names directly, to punish partial name matches over detailed path matches
        scorer=fuzz.ratio,
    )
    log.debug("Extracted after matching names: %r", matches)
    path_matches = process.extract(
        query,
        {id: str(project.path) for id, project in candidates.items()},
        limit=None,
        # Match tokens by order, to reflect the structure of paths
        scorer=fuzz.token_sort_ratio,
    )
    log.debug("Extracted after matching paths: %r", path_matches)
    matches.extend(path_matches)
    matches.sort(key=itemgetter(1))
    ids = list(dict.fromkeys(map(itemgetter(2), matches)))
    log.debug("Extracted after ordering by score: %r", ids)
    return ids


class DBusSearchProvider(dbus.service.Object):
    """
    A search provider

    See <https://developer.gnome.org/SearchProvider/>
    """

    _app: Gio.DesktopAppInfo
    _projects: dict[str, RecentProject]

    def __init__(
        self, busname, path, app: Gio.DesktopAppInfo, config_location: ConfigLocation
    ):
        super().__init__(busname, path)
        self._app = app
        self._projects = {}
        self._config = config_location
        self._log = log.getChild(self._app.get_id())

    def _make_id(self, project: RecentProject) -> str:
        return f"{self._app.get_id()}-{project.path.expanduser()}"

    def _refresh_projects(self):
        filepath = self._config.recent_projects()
        if filepath:
            self._projects = {
                self._make_id(p): p for p in read_recent_projects(filepath)
            }
        else:
            self._projects = {}
        self._log.debug("Got projects %r", self._projects)

    @search_provider_method(in_signature="as", out_signature="as")
    def GetInitialResultSet(self, terms: list[str]) -> list[str]:
        """
        GetInitialResultSet is called when a new search is started.

        It gets an array of search terms as arguments, and should return an
        array of result IDs. gnome-shell will call GetResultMetas for (some)
        of these result IDs to get details about the result that can be be
        displayed in the result list.
        """
        self._log.debug("Got query for %r", terms)
        self._log.info("Refreshing projects")
        self._refresh_projects()
        ids = find_matches(self._projects, terms)
        self._log.info("Found results for query %r: %r", terms, ids)
        return ids

    @search_provider_method(in_signature="asas", out_signature="as")
    def GetSubsearchResultSet(
        self, previous_results: list[str], terms: list[str]
    ) -> list[str]:
        """
        GetSubsearchResultSet is called to refine the initial search results
        when the user types more characters in the search entry.

        It gets the previous search results and the current search terms as
        arguments, and should return an array of result IDs, just like GetInitialResulSet.
        """
        self._log.debug("Got subquery for %r, after %r", terms, previous_results)
        ids = find_matches(
            {k: v for k, v in self._projects.items() if k in set(previous_results)},
            terms,
        )
        self._log.info(
            "Found results for query %r in %r: %r", terms, previous_results, ids
        )
        return ids

    @search_provider_method(in_signature="as", out_signature="aa{sv}")
    def GetResultMetas(self, results: list[str]) -> list[dict[str, Any]]:
        """
        GetResultMetas is called to obtain detailed information for results.

        It gets an array of result IDs as arguments, and should return a
        matching array of dictionaries (ie one a{sv} for each passed-in result ID).
        The following pieces of information should be provided for each result:

        - "id": the result ID
        - "name": the display name for the result
        - "icon": a serialized GIcon (see g_icon_serialize()), or alternatively,
        - "gicon": a textual representation of a GIcon (see g_icon_to_string()), or alternativly,
        - "icon-data": a tuple of type (iiibiiay) describing a pixbuf with width, height, rowstride, has-alpha, bits-per-sample, and image data
        - "description": an optional short description (1-2 lines)
        """
        self._log.debug("Requested metas for %r", results)
        projects = ((id, self._projects.get(id)) for id in results)
        return [
            {
                "id": id,
                "name": project.name,
                "description": str(project.path),
                "gicon": self._app.get_icon().to_string(),
            }
            for id, project in projects
            if project
        ]

    @search_provider_method(in_signature="sasu")
    def ActivateResult(self, id: str, terms: list[str], timestamp: int):
        """
        ActivateResult is called when the user clicks on an individual result to open it in the application.

        The arguments are the result ID, the current search terms and a timestamp.
        """
        self._log.debug("Activating %r", id)
        project = self._projects.get(id)
        if project:
            uri = project.path.as_uri()
            try:
                self._app.launch_uris([uri])
            except Exception as error:
                self._log.exception(
                    f"Failed to start app %s with %s: %s",
                    self._app.get_id(),
                    uri,
                    error,
                )

    @search_provider_method(in_signature="asu")
    def LaunchSearch(self, terms: list[str], timestamp: int):
        """
        LaunchSearch is called when the user clicks on the provider icon to
        display more search results in the application.

        The arguments are the current search terms and a timestamp.
        """
        self._log.debug("Launching for %r", terms)
        try:
            self._app.launch([], None)
        except Exception as error:
            self._log.exception(
                f"Failed to start app %s: %s", self._app.get_id(), error
            )


@dataclass
class ProviderDefinition:
    desktop_id: str
    relative_obj_path: str
    config: ConfigLocation


# Available candidates for search providers.
#
# Every candidate needs to have a corresponding search provider ini file in
# providers/ which matches the desktop ID and the object path.
# The object path must be unique for all declarations.
#
# The desktop ID denotes the app to launch in order to open projects, and the
# unique obj path links the provider declaration to the corresponding app here.
#
# This allows us to register a provider for each of the many ways Jetbrains
# products can be installed by (Jetbrains toolbox, Snap,
# some distribution packages, etc.)
PROVIDERS = [
    # Toolbox products
    ProviderDefinition(
        desktop_id="jetbrains-idea-ce.desktop",
        relative_obj_path="toolbox/ideace",
        config=ConfigLocation(vendor_dir="JetBrains", config_glob="IdeaIC*"),
    ),
    ProviderDefinition(
        desktop_id="jetbrains-webstorm.desktop",
        relative_obj_path="toolbox/webstorm",
        config=ConfigLocation(vendor_dir="JetBrains", config_glob="WebStorm*"),
    ),
]


def find_app(desktop_id: str) -> Optional[Gio.DesktopAppInfo]:
    try:
        # pygobject raises a type error if new returns NULL, for whatever reason
        return Gio.DesktopAppInfo.new(desktop_id)
    except TypeError:
        return None


def main():
    log.addHandler(JournalHandler())
    loglevel = (
        logging.DEBUG
        if "JETBRAINS_SEARCH_PROVIDER_LOG_DEBUG" in os.environ
        else logging.WARNING
    )
    log.setLevel(loglevel)

    try:
        log.info("Starting jetbrains-search-provider")

        dbus_loop = DBusGMainLoop()
        session_bus = dbus.SessionBus(mainloop=dbus_loop)
        name = dbus.service.BusName(
            "de.swsnr.searchprovider.Jetbrains", bus=session_bus
        )

        objects = []
        for provider_definition in PROVIDERS:
            app = find_app(provider_definition.desktop_id)
            if not app:
                continue
            path = f"/de/swsnr/searchprovider/jetbrains/{provider_definition.relative_obj_path}"
            log.info("Registering search provider for %s at %s", app.get_id(), path)
            object = DBusSearchProvider(
                busname=name,
                path=path,
                app=app,
                config_location=provider_definition.config,
            )
            objects.append(object)

        GLib.MainLoop().run()
    except Exception as error:
        log.exception(f"Failed to start jetbrains search provider: {error}")
        raise


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        pass
