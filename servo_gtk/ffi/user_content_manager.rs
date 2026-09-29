/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! C-ABI shims for [`UserContentManager`](crate::UserContentManager)
//! (`ServoGtkUserContentManager`).

use std::ffi::c_char;

use glib::gobject_ffi::GObject;
use glib::prelude::*;
use glib::translate::{IntoGlib, ToGlibPtr, from_glib_borrow};

use super::{GType, cstr_opt};
use crate::user_content::{UserContentManager, UserScript, UserStyleSheet};

/// `GType` of [`UserContentManager`](crate::UserContentManager).
#[unsafe(no_mangle)]
pub extern "C" fn servo_gtk_user_content_manager_get_type() -> GType {
    UserContentManager::static_type().into_glib()
}

/// Create a new, empty user content manager (transfer-full).
#[unsafe(no_mangle)]
pub extern "C" fn servo_gtk_user_content_manager_new() -> *mut GObject {
    let manager = UserContentManager::new();
    let obj: glib::Object = manager.upcast();
    obj.to_glib_full()
}

/// # Safety
/// `ptr` must be a valid `ServoGtkUserContentManager *`.
unsafe fn with_manager<R>(
    ptr: *mut GObject,
    f: impl FnOnce(&UserContentManager) -> R,
    default: R,
) -> R {
    if ptr.is_null() {
        return default;
    }
    let obj: glib::translate::Borrowed<glib::Object> = unsafe { from_glib_borrow(ptr) };
    match obj.downcast_ref::<UserContentManager>() {
        Some(manager) => f(manager),
        None => default,
    }
}

/// Inject a user script (by source) into pages loaded in the associated view.
///
/// # Safety
/// `manager` must be valid; `source` a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn servo_gtk_user_content_manager_add_script(
    manager: *mut GObject,
    source: *const c_char,
) {
    if let Some(source) = unsafe { cstr_opt(source) } {
        unsafe { with_manager(manager, |m| m.add_script(&UserScript::new(source)), ()) };
    }
}

/// Inject a user style sheet (by source).
///
/// # Safety
/// `manager` must be valid; `source` a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn servo_gtk_user_content_manager_add_style_sheet(
    manager: *mut GObject,
    source: *const c_char,
) {
    if let Some(source) = unsafe { cstr_opt(source) } {
        unsafe {
            with_manager(
                manager,
                |m| m.add_style_sheet(&UserStyleSheet::new(source)),
                (),
            )
        };
    }
}

/// Remove all previously added user scripts.
///
/// # Safety
/// `manager` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn servo_gtk_user_content_manager_remove_all_scripts(manager: *mut GObject) {
    unsafe { with_manager(manager, |m| m.remove_all_scripts(), ()) };
}

/// Remove all previously added user style sheets.
///
/// # Safety
/// `manager` must be valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn servo_gtk_user_content_manager_remove_all_style_sheets(
    manager: *mut GObject,
) {
    unsafe { with_manager(manager, |m| m.remove_all_style_sheets(), ()) };
}

/// Register a named script message handler.
///
/// # Safety
/// `manager` must be valid; `name` a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn servo_gtk_user_content_manager_register_script_message_handler(
    manager: *mut GObject,
    name: *const c_char,
) {
    if let Some(name) = unsafe { cstr_opt(name) } {
        unsafe { with_manager(manager, |m| m.register_script_message_handler(name), ()) };
    }
}

/// Unregister a previously registered named script message handler.
///
/// # Safety
/// `manager` must be valid; `name` a valid C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn servo_gtk_user_content_manager_unregister_script_message_handler(
    manager: *mut GObject,
    name: *const c_char,
) {
    if let Some(name) = unsafe { cstr_opt(name) } {
        unsafe { with_manager(manager, |m| m.unregister_script_message_handler(name), ()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glib::translate::*;

    #[test]
    fn user_content_manager_type_name_is_namespaced() {
        let ty = servo_gtk_user_content_manager_get_type();
        assert_ne!(ty, 0);
        let name = unsafe { glib::Type::from_glib(ty) }.name();
        assert_eq!(name, "ServoGtkUserContentManager");
    }
}
