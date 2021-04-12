// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::collections::HashMap;
use zbus::dbus_interface;
use zbus::export::zvariant::Value;

trait SearchProvider {}

struct DBusSearchProvider {}

#[dbus_interface(name = "org.zbus.MyGreeter1")]
impl DBusSearchProvider {
    fn get_initial_result_set(&mut self, terms: Vec<String>) -> Vec<String> {
        unimplemented!()
    }

    fn get_subsearch_result_set(
        &mut self,
        previous_results: Vec<String>,
        terms: Vec<String>,
    ) -> Vec<String> {
        unimplemented!()
    }

    fn get_result_metas(&mut self, results: Vec<String>) -> Vec<HashMap<String, Value>> {
        unimplemented!()
    }

    fn activate_result(&mut self, id: String, terms: Vec<String>, timestamp: u32) {
        unimplemented!()
    }

    fn launch_search(&mut self, terms: Vec<String>, timestamp: u32) {
        unimplemented!()
    }
}
