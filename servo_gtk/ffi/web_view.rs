/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! C-ABI shims for [`WebView`](crate::WebView) (`ServoGtkWebView`) and the
//! [`LoadEvent`](crate::LoadEvent) enum (`ServoGtkLoadEvent`).

use std::ffi::c_char;

use glib::gobject_ffi::GObject;
use glib::prelude::*;
use glib::translate::{IntoGlib, ToGlibPtr, from_glib_borrow, from_glib_none};

use super::{GType, cstr_opt};
use crate::user_content::UserContentManager;
use crate::web_view::{LoadEvent, WebView};

/// `GType` of [`WebView`](crate::WebView) (`ServoGtkWebView`).
#[unsafe(no_mangle)]
pub extern "C" fn servo_gtk_web_view_get_type() -> GType {
    WebView::static_type().into_glib()
}

/// `GType` of the [`LoadEvent`](crate::LoadEvent) enum (`ServoGtkLoadEvent`).
///
/// Exposed so GObject-Introspection can resolve the enum type used by the
/// #ServoGtkWebView::load-changed signal.
#[unsafe(no_mangle)]
pub extern "C" fn servo_gtk_load_event_get_type() -> GType {
    LoadEvent::static_type().into_glib()
}

/// Create a new [`WebView`](crate::WebView).
///
/// Returns a floating reference, transfer-full, as a `GtkWidget *`.
#[unsafe(no_mangle)]
pub extern "C" fn servo_gtk_web_view_new() -> *mut gtk::ffi::GtkWidget {
    let web_view = WebView::new();
    // Transfer full: hand ownership (the floating ref) to the caller.
    let obj: glib::Object = web_view.upcast();
    let ptr: *mut glib::gobject_ffi::GObject = obj.to_glib_full();
    ptr as *mut gtk::ffi::GtkWidget
}

/// Create a [`WebView`](crate::WebView) with an attached
/// [`UserContentManager`](crate::UserContentManager).
///
/// `manager` is borrowed (transfer-none). Returns a floating reference,
/// transfer-full.
///
/// # Safety
/// `manager` must be a valid `ServoGtkUserContentManager *` or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn servo_gtk_web_view_new_with_user_content_manager(
    manager: *mut GObject,
) -> *mut gtk::ffi::GtkWidget {
    if manager.is_null() {
        return servo_gtk_web_view_new();
    }
    let obj: glib::Object = unsafe { from_glib_none(manager) };
    let Some(manager) = obj.downcast_ref::<UserContentManager>() else {
        return std::ptr::null_mut();
    };
    let web_view = WebView::with_user_content_manager(manager);
    let obj: glib::Object = web_view.upcast();
    let ptr: *mut glib::gobject_ffi::GObject = obj.to_glib_full();
    ptr as *mut gtk::ffi::GtkWidget
}

/// Borrow a `ServoGtkWebView *` as a Rust [`WebView`], without taking a
/// reference.
///
/// # Safety
/// `ptr` must be a valid `ServoGtkWebView *`.
unsafe fn with_web_view<R>(
    ptr: *mut gtk::ffi::GtkWidget,
    f: impl FnOnce(&WebView) -> R,
    default: R,
) -> R {
    if ptr.is_null() {
        return default;
    }
    let obj: glib::translate::Borrowed<glib::Object> =
        unsafe { from_glib_borrow(ptr as *mut glib::gobject_ffi::GObject) };
    match obj.downcast_ref::<WebView>() {
        Some(view) => f(view),
        None => default,
    }
}

/// Load `url` in the web view.
///
/// # Safety
/// `web_view` must be a valid `ServoGtkWebView *`; `url` a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn servo_gtk_web_view_load_url(
    web_view: *mut gtk::ffi::GtkWidget,
    url: *const c_char,
) {
    if let Some(url) = unsafe { cstr_opt(url) } {
        unsafe { with_web_view(web_view, |v| v.load_url(url), ()) };
    }
}

/// Load an in-memory HTML document. `base_url` may be null.
///
/// # Safety
/// `web_view` must be a valid `ServoGtkWebView *`; `html` a valid C string;
/// `base_url` null or a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn servo_gtk_web_view_load_html(
    web_view: *mut gtk::ffi::GtkWidget,
    html: *const c_char,
    base_url: *const c_char,
) {
    if let Some(html) = unsafe { cstr_opt(html) } {
        let base_url = unsafe { cstr_opt(base_url) };
        unsafe { with_web_view(web_view, |v| v.load_html(html, base_url), ()) };
    }
}

/// Evaluate a snippet of JavaScript in the current page (fire-and-forget).
///
/// # Safety
/// `web_view` must be a valid `ServoGtkWebView *`; `script` a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn servo_gtk_web_view_evaluate_javascript(
    web_view: *mut gtk::ffi::GtkWidget,
    script: *const c_char,
) {
    if let Some(script) = unsafe { cstr_opt(script) } {
        unsafe { with_web_view(web_view, |v| v.evaluate_javascript(script), ()) };
    }
}

/// Reload the current page.
///
/// # Safety
/// `web_view` must be a valid `ServoGtkWebView *`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn servo_gtk_web_view_reload(web_view: *mut gtk::ffi::GtkWidget) {
    unsafe { with_web_view(web_view, |v| v.reload(), ()) };
}

/// Navigate back in session history.
///
/// # Safety
/// `web_view` must be a valid `ServoGtkWebView *`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn servo_gtk_web_view_go_back(web_view: *mut gtk::ffi::GtkWidget) {
    unsafe { with_web_view(web_view, |v| v.go_back(), ()) };
}

/// Navigate forward in session history.
///
/// # Safety
/// `web_view` must be a valid `ServoGtkWebView *`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn servo_gtk_web_view_go_forward(web_view: *mut gtk::ffi::GtkWidget) {
    unsafe { with_web_view(web_view, |v| v.go_forward(), ()) };
}

/// The URI of the currently loaded page, or null if nothing is loaded.
///
/// Returns a newly-allocated C string (transfer-full); free with `g_free()`.
///
/// # Safety
/// `web_view` must be a valid `ServoGtkWebView *`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn servo_gtk_web_view_get_uri(
    web_view: *mut gtk::ffi::GtkWidget,
) -> *mut c_char {
    unsafe {
        with_web_view(
            web_view,
            |v| match v.uri() {
                Some(uri) => uri.to_glib_full(),
                None => std::ptr::null_mut(),
            },
            std::ptr::null_mut(),
        )
    }
}

/// The title of the currently loaded page, or null.
///
/// Returns a newly-allocated C string (transfer-full); free with `g_free()`.
///
/// # Safety
/// `web_view` must be a valid `ServoGtkWebView *`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn servo_gtk_web_view_get_title(
    web_view: *mut gtk::ffi::GtkWidget,
) -> *mut c_char {
    unsafe {
        with_web_view(
            web_view,
            |v| match v.title() {
                Some(title) => title.to_glib_full(),
                None => std::ptr::null_mut(),
            },
            std::ptr::null_mut(),
        )
    }
}

/// Whether the view is currently loading a page.
///
/// # Safety
/// `web_view` must be a valid `ServoGtkWebView *`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn servo_gtk_web_view_get_is_loading(
    web_view: *mut gtk::ffi::GtkWidget,
) -> glib::ffi::gboolean {
    unsafe {
        with_web_view(
            web_view,
            |v| {
                if v.is_loading() {
                    glib::ffi::GTRUE
                } else {
                    glib::ffi::GFALSE
                }
            },
            glib::ffi::GFALSE,
        )
    }
}

/// The current page zoom level (1.0 == unzoomed).
///
/// # Safety
/// `web_view` must be a valid `ServoGtkWebView *`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn servo_gtk_web_view_get_zoom_level(
    web_view: *mut gtk::ffi::GtkWidget,
) -> f64 {
    unsafe { with_web_view(web_view, |v| v.zoom_level(), 1.0) }
}

/// Set the page zoom level (clamped to the range the engine accepts).
///
/// # Safety
/// `web_view` must be a valid `ServoGtkWebView *`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn servo_gtk_web_view_set_zoom_level(
    web_view: *mut gtk::ffi::GtkWidget,
    zoom_level: f64,
) {
    unsafe { with_web_view(web_view, |v| v.set_zoom_level(zoom_level), ()) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use glib::translate::*;

    #[test]
    fn web_view_type_name_is_namespaced() {
        gtk::init().ok();
        let ty = servo_gtk_web_view_get_type();
        assert_ne!(ty, 0, "ServoGtkWebView type should be registered");
        let name = unsafe { glib::Type::from_glib(ty) }.name();
        assert_eq!(name, "ServoGtkWebView");
    }
}
