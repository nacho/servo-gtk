/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! C-ABI (FFI) surface for `servo-gtk`.
//!
//! This is the hand-written FFI layer that exposes the library's GObjects
//! ([`WebView`](crate::WebView), [`UserContentManager`](crate::UserContentManager))
//! and the [`UserScript`](crate::UserScript) / [`UserStyleSheet`](crate::UserStyleSheet)
//! value types to C and to GObject-Introspection consumers.
//!
//! The functions here are thin shims over the Rust API: they marshal C strings
//! to/from Rust and delegate to the existing methods. The GObject *types*
//! themselves are registered by the glib-rs subclass machinery; the
//! `*_get_type()` functions simply surface the registered `GType` under a
//! stable, C-friendly symbol name so the hand-written C headers (which
//! `g-ir-scanner` reads) resolve against real symbols in the shared library.
//!
//! The per-class shims live in submodules:
//! - [`web_view`] — `ServoGtkWebView` and the `ServoGtkLoadEvent` enum.
//! - [`user_content_manager`] — `ServoGtkUserContentManager`.
//!
//! # Call ordering for C consumers
//!
//! A C `main()` MUST call [`servo_gtk_run_as_runner_if_requested`] as its very
//! first statement, then [`servo_gtk_init`] before creating any widgets:
//!
//! ```c
//! int main(int argc, char **argv) {
//!     servo_gtk_run_as_runner_if_requested();
//!     servo_gtk_init();
//!     // ... gtk_init(); create ServoGtkWebView; run the app ...
//! }
//! ```
//!
//! Signals are reachable from C via the standard GObject machinery
//! (`g_signal_connect`) using the registered names:
//! - `ServoGtkWebView::load-changed` — `(ServoGtkLoadEvent event)`
//! - `ServoGtkWebView::create-web-view` — `(const char *url)`
//! - `ServoGtkUserContentManager::script-message-received` —
//!   `(const char *name, const char *body)`

use std::ffi::{CStr, c_char, c_void};

#[path = "web_view.rs"]
pub mod web_view;

#[path = "user_content_manager.rs"]
pub mod user_content_manager;

/// GObject `GType`, matching C's `GType` (a `gsize`). This is `glib::ffi::GType`,
/// which is `usize` on this platform and ABI-compatible with C's `gsize`/`GType`.
pub(crate) type GType = glib::ffi::GType;

/// Convert a possibly-null C string to a Rust `&str`, returning `None` for null
/// or invalid UTF-8.
///
/// # Safety
/// `ptr` must be null or point to a valid NUL-terminated C string.
pub(crate) unsafe fn cstr_opt<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr) }.to_str().ok()
}

// ---------------------------------------------------------------------------
// Library init / runner hand-off
// ---------------------------------------------------------------------------

/// Hand off to the Servo runner subprocess if this process was spawned as one.
///
/// C consumers MUST call this as the very first statement in `main()`. When the
/// process was launched as the runner subprocess this runs the runner and
/// terminates the process without returning; otherwise it returns immediately.
#[unsafe(no_mangle)]
pub extern "C" fn servo_gtk_run_as_runner_if_requested() {
    crate::run_as_runner_if_requested();
}

/// Initialize `servo-gtk` for use from C: set up the OpenGL function loader and
/// initialize GTK.
///
/// This does two things C consumers need before creating a
/// [`WebView`](crate::WebView):
///
/// 1. Loads `libepoxy.so.0` and wires it into the `epoxy` GL loader so the
///    renderer can resolve GL entry points.
/// 2. Initializes GTK *through gtk4-rs*. `ServoGtkWebView` is a `GtkWidget`
///    subclass implemented in Rust; its class initialization asserts that GTK
///    was initialized through gtk4-rs (a plain C `gtk_init()`, including the one
///    `GtkApplication` performs, does not satisfy this because gtk4-rs records
///    its own main-thread state). Doing it here means any subsequent
///    `servo_gtk_web_view_new()` call from C succeeds.
///
/// Call this once, on the main thread, after
/// [`servo_gtk_run_as_runner_if_requested`] and before creating a `WebView`.
/// `gtk::init()` is idempotent, so this is safe to call alongside
/// `GtkApplication`.
///
/// Returns `TRUE` on success, `FALSE` if the GL loader or GTK could not be set
/// up.
#[unsafe(no_mangle)]
pub extern "C" fn servo_gtk_init() -> glib::ffi::gboolean {
    if let Err(err) = load_epoxy() {
        log::error!("servo_gtk_init: failed to load GL loader: {err}");
        return glib::ffi::GFALSE;
    }
    if gtk::init().is_err() {
        log::error!("servo_gtk_init: failed to initialize GTK");
        return glib::ffi::GFALSE;
    }
    glib::ffi::GTRUE
}

#[cfg(unix)]
fn load_epoxy() -> Result<(), String> {
    use std::ptr;
    let library = unsafe { libloading::os::unix::Library::new("libepoxy.so.0") }
        .map_err(|e| format!("dlopen libepoxy.so.0: {e}"))?;
    // Leak the library handle intentionally: the GL loader holds function
    // pointers into it for the lifetime of the process.
    let library: &'static libloading::os::unix::Library = Box::leak(Box::new(library));
    epoxy::load_with(|name| {
        unsafe { library.get::<*const c_void>(name.as_bytes()) }
            .map(|symbol| *symbol)
            .unwrap_or(ptr::null())
    });
    Ok(())
}

#[cfg(not(unix))]
fn load_epoxy() -> Result<(), String> {
    Err("GL loader bootstrap is only implemented for Unix".to_string())
}

/// Initialize GTK through gtk4-rs so its main-thread state is recorded.
///
/// This exists solely for the GObject-Introspection scan: the g-ir-scanner dump
/// program refs the #ServoGtkWebView (a #GtkWidget subclass) class, whose
/// gtk4-rs `class_init` asserts `gtk::is_initialized_main_thread()`. Calling the
/// C-level `gtk_init()` is not enough — gtk4-rs records its own main-thread id,
/// so GTK must be initialized through gtk4-rs. The library constructor in
/// `build-aux/gir-init-shim.c` calls this when `SERVO_GTK_GIR_SCAN` is set.
///
/// Not part of the public API; hidden from the version script.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "C" fn servo_gtk_ensure_gtk_init_for_scan() -> glib::ffi::gboolean {
    if gtk::init().is_ok() {
        glib::ffi::GTRUE
    } else {
        glib::ffi::GFALSE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_as_runner_returns_when_not_a_runner() {
        // In the test harness the process was not spawned as a runner, so this
        // must return rather than exit.
        servo_gtk_run_as_runner_if_requested();
    }
}
