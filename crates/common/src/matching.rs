// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Utilities for matching stuff.

use std::fmt::Debug;

use log::trace;

pub use indexmap::IndexMap;

/// Match against a list of terms and return a score.
pub trait ScoreMatchable {
    /// Match self against `terms` and return a score about how "well" self matches `terms`.
    ///
    /// A score of 0 or less denotes that `self` doesn't match `terms`; a score greater than zero indicates
    /// a match.
    ///
    /// The higher the score the better self matches `terms`; as a rule of thumb a score of 100 should be
    /// considered a perfect match.
    fn match_score<S: AsRef<str>>(&self, terms: &[S]) -> f64;
}

impl<'a, T> ScoreMatchable for &'a T
where
    T: ScoreMatchable,
{
    fn match_score<S: AsRef<str>>(&self, terms: &[S]) -> f64 {
        (*self).match_score(terms)
    }
}

/// Find all items from `items` which match the given `terms`.
///
/// `items` is an iterator over pairs of `(id, item)`.
///
/// For each item compute the score with `MatchScore`; discard projects with zero score,
/// and return a list of item IDs with non-zero score, ordered by score in descending order.
pub fn find_matching_items<'a, I, T, K, Item>(items: I, terms: &'a [T]) -> Vec<K>
where
    I: Iterator<Item = (K, Item)> + 'a,
    Item: ScoreMatchable,
    T: AsRef<str>,
    K: Debug,
{
    let mut matches: Vec<(f64, K)> = items
        .filter_map(move |(id, item)| {
            let score = item.match_score(terms);
            if 0.0 < score {
                Some((score, id))
            } else {
                None
            }
        })
        .collect();
    // Sort by score, descending
    matches.sort_by(|(score_a, _), (score_b, _)| score_b.partial_cmp(score_a).unwrap());
    trace!(
        "Matches {:?} for terms {:?}",
        matches,
        terms.iter().map(|s| s.as_ref()).collect::<Vec<&str>>()
    );
    matches.into_iter().map(move |(_, id)| id).collect()
}
