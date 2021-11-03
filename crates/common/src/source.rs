// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Sources for matchable items.

use std::fmt::Display;

use async_trait::async_trait;
use indexmap::map::IndexMap;

use crate::matching::ScoreMatchable;

/// A map of IDs to items which can be matched.
pub type IdMap<I> = IndexMap<String, I>;

/// A async source for matchable items.
#[async_trait]
pub trait AsyncItemsSource<T: ScoreMatchable> {
    /// The error
    type Err: Display;

    /// Find matchable items.
    async fn find_recent_items(&self) -> Result<IdMap<T>, Self::Err>;
}
