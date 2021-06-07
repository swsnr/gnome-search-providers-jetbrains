# Gnome search provider for Jetbrains projects

Add recent projects of various Jetbrains IDEs to Gnome search.

**Note:** This project is not affiliated with or endorsed by JetBrains.

![Screenshot](./screenshot.png)

Supports

- Android Studio (toolbox)
- CLion (toolbox)
- GoLand (toolbox)
- IDEA (toolbox)
- IDEA Community Edition (toolbox)
- PHPStorm (toolbox)
- PyCharm (toolbox)
- Rider (toolbox)
- RubyMine (toolbox)
- WebStorm (toolbox)

Under the hood this is a small systemd user service which implements the [search provider][1] DBus API and exposes recent projects from Jetbrains IDEs.

[1]: https://developer.gnome.org/SearchProvider/

## Installation

### Packages & binaries

- [AUR package](https://aur.archlinux.org/packages/gnome-search-providers-jetbrains/)

The following repositories are provided by 3rd parties, and listed only for informational purposes only.
I do **not** support or endorse these repositories, **use at your own risk.**

- [Fedora COPR](https://copr.fedorainfracloud.org/coprs/dontfreakout/gnome-search-providers-jetbrains/) by [dontfreakout](https://github.com/dontfreakout).


### From source

1. Install [rust](https://www.rust-lang.org/tools/install)

2. **Debian only:** Install `libgtk-3-dev`.

3. Build `make build`
4. Install `sudo make install`
   
   This installs to `/usr/local/`.

   **Note:** You really do need to install as `root`, system-wide.
   A per-user installation to `$HOME` does not work as of Gnome 40, because Gnome shell doesn't load search providers from `$HOME` (see <https://gitlab.gnome.org/GNOME/gnome-shell/-/issues/3060>).

## Uninstallation 

To uninstall use `sudo make uninstall`.

## License

Copyright Sebastian Wiesner <sebastian@swsnr.de>

This Source Code Form is subject to the terms of the Mozilla Public
License, v. 2.0. If a copy of the MPL was not distributed with this
file, You can obtain one at <http://mozilla.org/MPL/2.0/>.
