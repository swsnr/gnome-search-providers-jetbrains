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


import os
import logging
from systemd.journal import JournalHandler
from gi.repository import GLib


log = logging.getLogger("jetbrains-search-provider")


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
        GLib.MainLoop().run()
    except Exception as error:
        log.exception(f"Failed to start jetbrains search provider: {error}")
        raise


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        pass
