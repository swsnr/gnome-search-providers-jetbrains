# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project doesn't really care for versioning.

## [Unreleased]

## [1.15.0] – 2023-09-25

### Changed
- Refactored internals a lot to simplify code.

### Fixed
- Correctly read project names.

### Removed
- Remove `LogControl` interface, as I never needed it or used it in fact.

## [1.14.0] – 2023-09-14

### Added
- Add support for RustRover (see [GH-49]).

[GH-49]: https://github.com/swsnr/gnome-search-providers-jetbrains/pull/49


## [1.13.0] – 2023-09-10

### Changed
- Update all dependencies.
- Do not update recent projects when starting a search; instead update all recent projects at startup or when explicitly requested over DBus (see [GH-48]).
  This speeds up initial search and makes sure the search provider answers in time.
- `systemctl --user reload gnome-search-providers-jetbrains.service` refreshes all recent projects (see [GH-48]).

[GH-48]: https://github.com/swsnr/gnome-search-providers-jetbrains/pull/48

## [1.12.6] – 2023-08-06

### Fixed
- Fix bindir in Makefile.

## [1.12.5] – 2023-08-06

### Changed
- Update all dependencies.
- The makefile now installs the binary to the bindir, e.g `/usr/bin/` or `/usr/local/bin/`.

## [1.12.4] – 2023-03-06

### Fixed
- No longer deadlock at startup while registering interfaces.

## [1.12.3] – 2023-02-27

### Changed
- Update dependencies.

## [1.12.2] – 2022-12-01

### Changed
- Update repository URL to <https://github.com/swsnr/gnome-search-providers-jetbrains>.

## [1.12.1] – 2022-11-24

### Changed

- Update dependencies.

### Fixed

- Fix automatic update to journald logging (see [GH-35]).

[GH-35]: https://github.com/swsnr/gnome-search-providers-jetbrains/pull/35

## [1.12.0] – 2022-10-12

### Changed

- Migrate back to <https://github.com/swsnr/gnome-search-providers-jetbrains>.
- Update dependencies.

## [1.11.2] – 2022-05-30

### Fixed

- Release common crate as well.

## [1.11.1] – 2022-05-30

### Changed

- Update dependencies.

## [1.11.0] – 2022-02-24

### Changed

- Drop `async_trait` dependency.

## [1.10.0] – 2022-02-03

### Changed

- Migrate to <https://codeberg.org/flausch/gnome-search-providers-jetbrains>.
- Return proper syslog identifier from systemd log control interface.
- Update dependencies, and remove a few redundant dependencies.

## [1.9.1] – 2022-01-12

### Fixed

- Remove target dependencies from Makefile to simplify manual installation.

## [1.9.0] – 2022-01-10

### Added
- Implement `LogControl` DBus interface, in order to set the log level with `systemctl service-log-level` (see [GH-27]).

### Changed
- Update dependencies.
- Use `tracing` for logging.
- Rename `de.swsnr.searchprovider.Jetbrains.service` to `gnome-search-providers-jetbrains.service`.

[GH-27]: https://github.com/swsnr/gnome-search-providers-jetbrains/pull/27

## [1.8.0] – 2021-11-24

### Changed
- Make internal async, to account for changes in zbus' APIs and to hopefully speed up things a bit.

## [1.7.1] – 2021-09-08

### Fixed
- Properly consider log level for journald logging.

## [1.7.0] – 2021-09-08

### Fixed
- Fix handling of DBus message.

## [1.6.0] – 2021-08-28

**This release is broken, do not use.**

### Added
- Automatically detect whether to log to the systemd journal.
- Improve debug logging.

### Removed
- Remove `--journal-log` flag.

## [1.5.0] – 2021-05-25

### Added
- Enable debug logging if `$LOG_DEBUG` is set (see [GH-17]).
- Add `--journal-log` to log directly to the systemd journal; this adds a dependency on the `systemd` crate and, by implication, libsystemd (see [GH-17]).
  Consequently this program no longer builds if systemd is not installed.

### Changed
- The systemd service now logs directly to the systemd journal; this improves representation of log levels in logging (see [GH-17]).
- Rename scopes for running IDE instances to clarify the origin of the scope, and distinguish apps started from the search provider from apps started by Gnome Shell.

[GH-17]: https://github.com/swsnr/gnome-search-providers-jetbrains/pull/17

## [1.4.1] – 2021-05-19

### Fixed

- Correctly parse recent solutions from Rider (see [GH-12] and [GH-13], thanks [axelgenus]).

[GH-12]: https://github.com/swsnr/gnome-search-providers-jetbrains/issues/12
[GH-13]: https://github.com/swsnr/gnome-search-providers-jetbrains/pull/13

[axelgenus]: https://github.com/axelgenus

## [1.4.0] – 2021-05-16

### Changed
- Move launched processes to new `app-gnome` systemd scopes, like Gnome itself does when starting applications:
  - Prevents systemd from killing IDE processes launched by the search provider when stopping the search provider service (see below).
  - Improves resource control, because systemd now tracks resource usage of launched IDEs separate from the search provider.
    * This improves interaction with e.g. systemd-oomd because the search provider no longer implicitly aggregates resource usage of all launched IDEs; thus when running into a OOM situation after launching many IDEs through the search provider systemd-oomd will only kill specific IDEs with excessive memory consumption not all IDEs and the search provider at once.

### Fixed
- Correctly detect overridden names of projects.
- No longer quit application instances launched by the search provider when stopping the search provider service; the search provider now moves processes to new systemd scopes to prevent this.

## [1.3.0] – 2021-04-25

### Changed

- Improve order of matches:
  - Rank matches in the project name higher than matches in the path, and
  - rank path matches by position of term in match (the more to the right the better the term matched the more specific segments of the path).

## [1.2.3] – 2021-04-23

### Fixed

- Fix `make install` with parallel make, by setting a proper dependency on the `build` target.

## [1.2.2] – 2021-04-22

### Fixed

- Exit with failure if the desired bus name is already owned by another process.
- Substitute prefix in service files during `make build` and `make install`.

## [1.2.1] — 2021-04-16

### Fixed

- Fix `Cargo.lock`.

## [1.2.0] — 2021-04-16

### Changed

- Search case-insensitive.

### Fixed

- Add missing directory separate in `make uninstall` to remove search provider configurations properly.
- Remove redundant `Environment` stanza from systemd unit.

## [1.1.0] – 2021-04-15

### Added

- Add all toolbox products (thanks [atomicptr], see [GH-3] and [GH-6])

### Changed

- Rewritten in Rust; reduces runtime dependencies, but requires a Rust installation to build.

### Fixed

- Fix overly lax matching, by replacing fuzzy searching with strict substring matching (see [GH-7]).

[atomicptr]: https://github.com/atomicptr
[GH-3]: https://github.com/swsnr/gnome-search-providers-jetbrains/issues/3
[GH-6]: https://github.com/swsnr/gnome-search-providers-jetbrains/pull/6
[GH-7]: https://github.com/swsnr/gnome-search-providers-jetbrains/issues/7

## [1] – 2020-04-11

Initial prototype in Python, with support for Toolbox IDEA CE and WebStorm.

[Unreleased]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.15.0...HEAD
[1.15.0]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.14.0...v1.15.0
[1.14.0]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.13.0...v1.14.0
[1.13.0]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.12.6...v1.13.0
[1.12.6]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.12.5...v1.12.6
[1.12.5]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.12.4...v1.12.5
[1.12.4]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.12.3...v1.12.4
[1.12.3]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.12.2...v1.12.3
[1.12.2]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.12.1...v1.12.2
[1.12.1]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.12.0...v1.12.1
[1.12.0]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.11.2...v1.12.0
[1.11.2]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.11.1...v1.11.2
[1.11.1]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.11.0...v1.11.1
[1.11.0]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.10.0...v1.11.0
[1.10.0]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.9.1...v1.10.0
[1.9.1]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.9.0...v1.9.1
[1.9.0]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.8.0...v1.9.0
[1.8.0]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.7.1...v1.8.0
[1.7.1]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.7.0...v1.7.1
[1.7.0]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.6.0...v1.7.0
[1.6.0]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.5.0...v1.6.0
[1.5.0]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.4.1...v1.5.0
[1.4.1]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.4.0...v1.4.1
[1.4.0]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.3.0...v1.4.0
[1.3.0]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.2.3...v1.3.0
[1.2.3]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.2.2...v1.2.3
[1.2.2]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.2.1...v1.2.2
[1.2.1]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/swsnr/gnome-search-providers-jetbrains/compare/v1...v1.1.0
[1]: https://github.com/swsnr/gnome-search-providers-jetbrains/releases/tag/v1
