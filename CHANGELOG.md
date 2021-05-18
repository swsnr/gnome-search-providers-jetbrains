# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project doesn't really care for versioning.

## [Unreleased]

### Fixed

- Correctly parse recent solutions from Rider (see [GH-12] and [GH-13], thanks [axelgenus]).

[GH-12]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/issues/12 
[GH-13]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/pull/13 

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
[GH-3]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/issues/3
[GH-6]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/pull/6
[GH-7]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/issues/7

## [1] – 2020-04-11

Initial prototype in Python, with support for Toolbox IDEA CE and WebStorm.

[Unreleased]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/compare/v1.4.0...HEAD
[1.4.0]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/compare/v1.3.0...v1.4.0
[1.3.0]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/compare/v1.2.3...v1.3.0
[1.2.3]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/compare/v1.2.2...v1.2.3
[1.2.2]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/compare/v1.2.1...v1.2.2
[1.2.1]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/compare/v1...v1.1.0
[1]: https://github.com/lunaryorn/gnome-search-providers-jetbrains/releases/tag/v1
