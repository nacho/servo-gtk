/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#ifndef SERVO_GTK_USER_CONTENT_MANAGER_H
#define SERVO_GTK_USER_CONTENT_MANAGER_H

#include <glib-object.h>

G_BEGIN_DECLS

#define SERVO_GTK_TYPE_USER_CONTENT_MANAGER (servo_gtk_user_content_manager_get_type())

/**
 * ServoGtkUserContentManager:
 *
 * Manages user content (scripts and style sheets) injected into pages loaded in
 * a #ServoGtkWebView, and a page-to-native message channel. Modelled on
 * WebKitGTK's `WebKitUserContentManager`.
 *
 * Create one with servo_gtk_user_content_manager_new() and pass it to
 * servo_gtk_web_view_new_with_user_content_manager(). User scripts and style
 * sheets are added by source string with
 * servo_gtk_user_content_manager_add_script() and
 * servo_gtk_user_content_manager_add_style_sheet(); they take effect on the next
 * page load.
 *
 * After registering a named handler with
 * servo_gtk_user_content_manager_register_script_message_handler(), injected
 * page JavaScript can call
 * `window.servoGtk.messageHandlers.<name>.postMessage(value)`; the value is
 * delivered via the #ServoGtkUserContentManager::script-message-received signal
 * `(const char *name, const char *body)` where @body is the JSON serialization
 * of @value.
 */
G_DECLARE_FINAL_TYPE (ServoGtkUserContentManager,
                      servo_gtk_user_content_manager,
                      SERVO_GTK,
                      USER_CONTENT_MANAGER,
                      GObject)

/**
 * servo_gtk_user_content_manager_new:
 *
 * Creates a new, empty #ServoGtkUserContentManager.
 *
 * Returns: (transfer full): a new #ServoGtkUserContentManager
 */
ServoGtkUserContentManager *servo_gtk_user_content_manager_new (void);

/**
 * servo_gtk_user_content_manager_add_script:
 * @self: a #ServoGtkUserContentManager
 * @source: the JavaScript source of the user script
 *
 * Injects a user script into pages loaded in the associated #ServoGtkWebView.
 * Takes effect on the next page load.
 */
void servo_gtk_user_content_manager_add_script (ServoGtkUserContentManager *self,
                                                const char                 *source);

/**
 * servo_gtk_user_content_manager_add_style_sheet:
 * @self: a #ServoGtkUserContentManager
 * @source: the CSS source of the user style sheet
 *
 * Injects a user style sheet into pages loaded in the associated
 * #ServoGtkWebView. Takes effect on the next page load.
 */
void servo_gtk_user_content_manager_add_style_sheet (ServoGtkUserContentManager *self,
                                                     const char                 *source);

/**
 * servo_gtk_user_content_manager_remove_all_scripts:
 * @self: a #ServoGtkUserContentManager
 *
 * Removes all user scripts previously added to @self.
 */
void servo_gtk_user_content_manager_remove_all_scripts (ServoGtkUserContentManager *self);

/**
 * servo_gtk_user_content_manager_remove_all_style_sheets:
 * @self: a #ServoGtkUserContentManager
 *
 * Removes all user style sheets previously added to @self.
 */
void servo_gtk_user_content_manager_remove_all_style_sheets (ServoGtkUserContentManager *self);

/**
 * servo_gtk_user_content_manager_register_script_message_handler:
 * @self: a #ServoGtkUserContentManager
 * @name: the handler name
 *
 * Registers a named script message handler. Takes effect on the next page load.
 */
void servo_gtk_user_content_manager_register_script_message_handler (ServoGtkUserContentManager *self,
                                                                     const char                 *name);

/**
 * servo_gtk_user_content_manager_unregister_script_message_handler:
 * @self: a #ServoGtkUserContentManager
 * @name: the handler name
 *
 * Unregisters a previously registered named script message handler. Existing
 * pages keep the handler until reloaded.
 */
void servo_gtk_user_content_manager_unregister_script_message_handler (ServoGtkUserContentManager *self,
                                                                       const char                 *name);

G_END_DECLS

#endif /* SERVO_GTK_USER_CONTENT_MANAGER_H */
