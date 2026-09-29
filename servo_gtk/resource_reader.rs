/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Supplies Servo's built-in resources (HSTS preload list, public suffix list,
//! the debugger script, ...) to the engine.
//!
//! Servo requires a resource reader to be registered before the constellation
//! starts; otherwise it panics with "No resource reader registered". Servo's
//! own `servo-default-resources` crate registers a default reader via
//! `inventory`, but that link-section registration is garbage-collected when
//! the crate is linked through our `staticlib` into `libservo-gtk.so` (nothing
//! references the registration object, so the linker never pulls it in and its
//! constructor never runs). We therefore disable Servo's `baked-in-resources`
//! feature and register our own reader here, from an object that is guaranteed
//! to be linked because the runner references it (see [`ensure_registered`]).
//!
//! The resources are compiled into a GResource bundle at build time (see
//! `build.rs`) and embedded in the binary, so no files need to be installed
//! alongside the library.

use gio::Resource;
use glib::Bytes;
use servo::resources::{Resource as ServoResource, ResourceReaderMethods};
use std::path::PathBuf;
use std::sync::Once;

struct ResourceReaderInstance;

unsafe impl Send for ResourceReaderInstance {}
unsafe impl Sync for ResourceReaderInstance {}

static RESOURCE_READER: ResourceReaderInstance = ResourceReaderInstance;

// Register our reader with Servo. This is a link-time (`inventory`) submission;
// it takes effect as long as this object is linked into the final artifact,
// which [`ensure_registered`] guarantees.
servo::submit_resource_reader!(&RESOURCE_READER);

/// Force this module (and thus the `submit_resource_reader!` registration above)
/// to be linked into the final artifact, and register the GResource bundle with
/// GIO so the reader can serve it.
///
/// The runner calls this before starting Servo. The reference to
/// [`RESOURCE_READER`] prevents the linker from garbage-collecting the
/// registration when servo-gtk is linked as a static library into
/// `libservo-gtk.so`.
pub(crate) fn ensure_registered() {
    static REGISTER_GRESOURCE: Once = Once::new();
    REGISTER_GRESOURCE.call_once(|| {
        let resource_data = include_bytes!(concat!(env!("OUT_DIR"), "/resources.gresource"));
        let bytes = Bytes::from_static(resource_data);
        let resource = Resource::from_data(&bytes).expect("Failed to load gresource");
        gio::resources_register(&resource);
    });
    // Touch the static so the registration object cannot be dropped.
    std::hint::black_box(&RESOURCE_READER);
}

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
