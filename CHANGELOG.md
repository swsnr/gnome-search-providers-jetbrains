# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project doesn't really care for versioning.

## [Unreleased]

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
[GH-3]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/issues/3
[GH-6]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/pull/6
[GH-7]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/issues/7

## [1] – 2020-04-11

Initial prototype in Python, with support for Toolbox IDEA CE and WebStorm.

[Unreleased]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/compare/v1.2.2...HEAD
[1.2.1]: https://github.com/olivierlacan/keep-a-changelog/compare/v1.2.1...v1.2.2
[1.2.1]: https://github.com/olivierlacan/keep-a-changelog/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/olivierlacan/keep-a-changelog/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/olivierlacan/keep-a-changelog/compare/v1...v1.1.0
[1]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/releases/tag/v1
