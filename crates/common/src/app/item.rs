// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Items to be launch by an app.

use crate::matching::ScoreMatchable;
use log::trace;

/// A target for launching an app.
#[derive(Debug, PartialEq, Clone)]
pub enum AppLaunchTarget {
    /// A URI to launch.
    Uri(String),
    /// A file to launch.
    File(String),
}

impl AppLaunchTarget {
    /// The description for this launch target.
    pub fn description(&self) -> &str {
        match self {
            AppLaunchTarget::Uri(uri) => uri,
            AppLaunchTarget::File(path) => path,
        }
    }
}

/// A recent item from the file system.
#[derive(Debug, PartialEq)]
pub struct AppLaunchItem {
    /// The human readable name
    pub name: String,

    /// The target to launch when activating this item.
    pub target: AppLaunchTarget,
}

impl ScoreMatchable for AppLaunchItem {
    /// Compute the score of matching self against `terms`.
    ///
    /// If all terms match the name each term contributes a score of 10; this makes sure
    /// that precise matches in the name boost the score somewhat to the top.
    ///
    /// If all terms match the target each term contributes 1 to score, scaled by the relative position
    /// of the right-most match, assuming that paths typically go from least to most specific segment,
    /// to the farther to the right a term matches the more specific it was.
    fn match_score<S: AsRef<str>>(&self, terms: &[S]) -> f64 {
        let name = self.name.to_lowercase();
        let target = match &self.target {
            AppLaunchTarget::Uri(uri) => uri,
            AppLaunchTarget::File(file) => file,
        }
        .to_lowercase();
        let name_score = terms.iter().try_fold(0.0, |score, term| {
            name.contains(&term.as_ref().to_lowercase())
                .then(|| score + 10.0)
                .ok_or(())
        });
        let target = terms.iter().try_fold(0.0, |score, term| {
            target
                .rfind(&term.as_ref().to_lowercase())
                .ok_or(())
                .map(|index| score + 1.0 * (index as f64 / target.len() as f64))
        });
        let score = name_score.unwrap_or_default() + target.unwrap_or_default();
        trace!(
            "Item {:?} matches terms {:?} with score {} (name={:?}, target={:?})",
            self,
            terms.iter().map(|s| s.as_ref()).collect::<Vec<&str>>(),
            score,
            name_score,
            target
        );
        score
    }
}
