// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Matching things from filesystems.

use crate::{RecentItem, ScoreMatchable};

/// A recent item from the file system.
#[derive(Debug, PartialEq)]
pub struct RecentFileSystemItem {
    /// The human readable name
    pub name: String,
    /// The path on the file system.
    ///
    /// The path doesn't need to be a filesystem path, strictly, as long as it's something
    /// that resembles a path, e.g. a URL.
    pub path: String,
}

impl ScoreMatchable for RecentFileSystemItem {
    /// Compute the score of matching self against `terms`.
    ///
    /// If all terms match the name each term contributes a score of 10; this makes sure
    /// that precise matches in the name boost the score somewhat to the top.
    ///
    /// If all terms match the path each term contributes 1 to score, scaled by the relative position
    /// of the right-most match, assuming that paths typically go from least to most specific segment,
    /// to the farther to the right a term matches the more specific it was.
    fn match_score<S: AsRef<str>>(&self, terms: &[S]) -> f64 {
        let name = self.name.to_lowercase();
        let path = self.path.to_lowercase();
        let name_score = terms.iter().try_fold(0.0, |score, term| {
            name.contains(&term.as_ref().to_lowercase())
                .then(|| score + 10.0)
                .ok_or(())
        });
        let path_score = terms.iter().try_fold(0.0, |score, term| {
            path.rfind(&term.as_ref().to_lowercase())
                .ok_or(())
                .map(|index| score + 1.0 * (index as f64 / path.len() as f64))
        });
        name_score.unwrap_or_default() + path_score.unwrap_or_default()
    }
}

impl RecentItem for RecentFileSystemItem {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.path
    }

    fn uri(&self) -> &str {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    mod search {
        use crate::{find_matching_items, RecentFileSystemItem};

        fn do_match<'a>(items: &[(&'a str, RecentFileSystemItem)], terms: &[&str]) -> Vec<&'a str> {
            find_matching_items(items.iter().map(|(s, p)| (*s, p)), terms)
        }

        #[test]
        fn matches_something() {
            let projects = vec![(
                "foo",
                RecentFileSystemItem {
                    name: "mdcat".to_string(),
                    path: "/home/foo/dev/mdcat".to_string(),
                },
            )];
            assert_eq!(do_match(&projects, &["mdcat"]), ["foo"]);
        }

        /// Regression test for https://github.com/lunaryorn/gnome-search-providers-jetbrains/issues/7
        #[test]
        fn do_not_find_undesired_projects() {
            let projects = vec![
                (
                    "foo-1",
                    RecentFileSystemItem {
                        name: "ui-pattern-library".to_string(),
                        path: "/home/foo/dev/something/ui-pattern-library".to_string(),
                    },
                ),
                (
                    "foo-2",
                    RecentFileSystemItem {
                        name: "dauntless-builder".to_string(),
                        path: "/home/foo/dev/dauntless-builder".to_string(),
                    },
                ),
                (
                    "foo-3",
                    RecentFileSystemItem {
                        name: "typo3-ssr".to_string(),
                        path: "/home/foo/dev/something/typo3-ssr".to_string(),
                    },
                ),
            ];
            assert!(do_match(&projects, &["flutter_test_app"]).is_empty());
        }

        #[test]
        fn ignore_case_of_name() {
            let projects = vec![(
                "foo",
                RecentFileSystemItem {
                    name: "mdCat".to_string(),
                    path: "/home/foo/dev/foo".to_string(),
                },
            )];
            assert_eq!(do_match(&projects, &["Mdcat"]), ["foo"]);
        }

        #[test]
        fn ignore_case_of_path() {
            let projects = vec![(
                "foo",
                RecentFileSystemItem {
                    name: "bar".to_string(),
                    path: "/home/foo/dev/mdcaT".to_string(),
                },
            )];
            assert_eq!(do_match(&projects, &["Mdcat"]), ["foo"]);
        }

        #[test]
        fn matches_in_name_rank_higher() {
            let projects = vec![
                (
                    "1",
                    RecentFileSystemItem {
                        name: "bar".to_string(),
                        // This matches foo as well because of /home/foo
                        path: "/home/foo/dev/bar".to_string(),
                    },
                ),
                (
                    "2",
                    RecentFileSystemItem {
                        name: "foo".to_string(),
                        path: "/home/foo/dev/foo".to_string(),
                    },
                ),
            ];
            assert_eq!(do_match(&projects, &["foo"]), ["2", "1"]);
        }

        #[test]
        fn matches_at_end_of_path_rank_higher() {
            let projects = vec![
                (
                    "1",
                    RecentFileSystemItem {
                        name: "p1".to_string(),
                        // This matches foo as well because of /home/foo
                        path: "/home/foo/dev/bar".to_string(),
                    },
                ),
                (
                    "2",
                    RecentFileSystemItem {
                        name: "p1".to_string(),
                        path: "/home/foo/dev/foo".to_string(),
                    },
                ),
            ];
            assert_eq!(do_match(&projects, &["foo"]), ["2", "1"]);
        }
    }
}
