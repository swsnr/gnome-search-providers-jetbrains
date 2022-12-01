// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Apps and their recent items.

mod item;
mod launchservice;
mod provider;

pub use item::AppLaunchItem;
pub use launchservice::{App, AppId, AppLaunchClient, AppLaunchService, SystemdScopeSettings};
pub use provider::{AppItemSearchProvider, AppItemSearchRequest};

#[cfg(test)]
mod tests {
    mod search {
        use pretty_assertions::assert_eq;

        use crate::app::AppLaunchItem;
        use crate::matching::find_matching_items;

        fn do_match<'a>(items: &[(&'a str, AppLaunchItem)], terms: &[&str]) -> Vec<&'a str> {
            find_matching_items(items.iter().map(|(s, p)| (*s, p)), terms)
        }

        #[test]
        fn matches_something() {
            let items = vec![(
                "foo",
                AppLaunchItem {
                    name: "mdcat".to_string(),
                    uri: "/home/foo/dev/mdcat".to_string(),
                },
            )];
            assert_eq!(do_match(&items, &["mdcat"]), ["foo"]);
        }

        /// Regression test for https://github.com/swsnr/gnome-search-providers-jetbrains/issues/7
        #[test]
        fn do_not_find_undesired_items() {
            let items = vec![
                (
                    "foo-1",
                    AppLaunchItem {
                        name: "ui-pattern-library".to_string(),
                        uri: "/home/foo/dev/something/ui-pattern-library".to_string(),
                    },
                ),
                (
                    "foo-2",
                    AppLaunchItem {
                        name: "dauntless-builder".to_string(),
                        uri: "/home/foo/dev/dauntless-builder".to_string(),
                    },
                ),
                (
                    "foo-3",
                    AppLaunchItem {
                        name: "typo3-ssr".to_string(),
                        uri: "/home/foo/dev/something/typo3-ssr".to_string(),
                    },
                ),
            ];
            assert!(do_match(&items, &["flutter_test_app"]).is_empty());
        }

        #[test]
        fn ignore_case_of_name() {
            let items = vec![(
                "foo",
                AppLaunchItem {
                    name: "mdCat".to_string(),
                    uri: "/home/foo/dev/foo".to_string(),
                },
            )];
            assert_eq!(do_match(&items, &["Mdcat"]), ["foo"]);
        }

        #[test]
        fn ignore_case_of_path() {
            let items = vec![(
                "foo",
                AppLaunchItem {
                    name: "bar".to_string(),
                    uri: "/home/foo/dev/mdcaT".to_string(),
                },
            )];
            assert_eq!(do_match(&items, &["Mdcat"]), ["foo"]);
        }

        #[test]
        fn matches_in_name_rank_higher() {
            let items = vec![
                (
                    "1",
                    AppLaunchItem {
                        name: "bar".to_string(),
                        // This matches foo as well because of /home/foo
                        uri: "/home/foo/dev/bar".to_string(),
                    },
                ),
                (
                    "2",
                    AppLaunchItem {
                        name: "foo".to_string(),
                        uri: "/home/foo/dev/foo".to_string(),
                    },
                ),
            ];
            assert_eq!(do_match(&items, &["foo"]), ["2", "1"]);
        }

        #[test]
        fn matches_at_end_of_path_rank_higher() {
            let items = vec![
                (
                    "1",
                    AppLaunchItem {
                        name: "p1".to_string(),
                        // This matches foo as well because of /home/foo
                        uri: "/home/foo/dev/bar".to_string(),
                    },
                ),
                (
                    "2",
                    AppLaunchItem {
                        name: "p1".to_string(),
                        uri: "/home/foo/dev/foo".to_string(),
                    },
                ),
            ];
            assert_eq!(do_match(&items, &["foo"]), ["2", "1"]);
        }
    }
}
