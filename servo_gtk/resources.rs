/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use gio::Resource;
use glib::Bytes;
use servo::resources::{Resource as ServoResource, ResourceReaderMethods};
use std::path::PathBuf;

pub struct ResourceReaderInstance;

impl Default for ResourceReaderInstance {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceReaderInstance {
    pub fn new() -> Self {
        let resource_data = include_bytes!(concat!(env!("OUT_DIR"), "/resources.gresource"));
        let bytes = Bytes::from_static(resource_data);
        let resource = Resource::from_data(&bytes).expect("Failed to load gresource");
        gio::resources_register(&resource);

        Self
    }
}

unsafe impl Send for ResourceReaderInstance {}
unsafe impl Sync for ResourceReaderInstance {}

impl ResourceReaderMethods for ResourceReaderInstance {
    fn read(&self, res: ServoResource) -> Vec<u8> {
        let path = format!("/com/servo-gtk/{}", res.filename());

        let bytes = gio::resources_lookup_data(&path, gio::ResourceLookupFlags::NONE)
            .unwrap_or_else(|_| panic!("Failed to read resource {path}"));
        bytes.to_vec()
    }

    fn sandbox_access_files(&self) -> Vec<PathBuf> {
        vec![]
    }

    fn sandbox_access_files_dirs(&self) -> Vec<PathBuf> {
        vec![]
    }
}
