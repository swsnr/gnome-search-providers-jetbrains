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
- [Fedora RPM](https://copr.fedorainfracloud.org/coprs/dontfreakout/gnome-search-providers-jetbrains/)

### From source

Install [rust](https://www.rust-lang.org/tools/install) then run

```console
$ make build
$ sudo make install
```

This install to `/usr/local/`.

**Note:** You really do need to install as `root`, system-wide.
A per-user installation to `$HOME` does not work as of Gnome 40, because Gnome shell doesn't load search providers from `$HOME` (see <https://gitlab.gnome.org/GNOME/gnome-shell/-/issues/3060>).

To uninstall use `sudo make uninstall`.

## License

Copyright Sebastian Wiesner <sebastian@swsnr.de>

This Source Code Form is subject to the terms of the Mozilla Public
License, v. 2.0. If a copy of the MPL was not distributed with this
file, You can obtain one at <http://mozilla.org/MPL/2.0/>.
