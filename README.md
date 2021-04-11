# Gnome search provider for Jetbrains projects

Add recent projects of various Jetbrains IDEs to Gnome search.

**Note:** This project is not affiliated with or endorsed by JetBrains.

![Screenshot](./screenshot.png)

Supports

- IDEA Community (Jetbrains toolbox)
- Webstorm (Jetbrains toolbox)

Under the hood this is a small systemd user service which implements the [search provider][1] DBus API and exposes recent projects from Jetbrains IDEs.

[1]: https://developer.gnome.org/SearchProvider/

## Installation

For Arch Linux there's an [AUR package][2].

Install all requirements (see below), then run `sudo make install`.

The DBus service is activatable; hence you don't need to `systemd enable` any service.

To uninstall use `sudo make uninstall`.

[2]: https://aur.archlinux.org/packages/gnome-search-providers-jetbrains/

## Requirements

- [pygobject](https://pygobject.readthedocs.io/en/latest/getting_started.html)
- [python-systemd](https://github.com/systemd/python-systemd)
- [fuzzywuzzy](https://github.com/seatgeek/fuzzywuzzy)

## License

Copyright Sebastian Wiesner <sebastian@swsnr.de>

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at <http://www.apache.org/licenses/LICENSE-2.0>.

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
