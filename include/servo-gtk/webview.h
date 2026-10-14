/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#ifndef SERVO_GTK_WEB_VIEW_H
#define SERVO_GTK_WEB_VIEW_H

#include <gtk/gtk.h>

#include "enums.h"
#include "usercontentmanager.h"

G_BEGIN_DECLS

#define SERVO_GTK_TYPE_WEB_VIEW (servo_gtk_web_view_get_type())

/**
 * ServoGtkWebView:
 *
 * A GTK widget that embeds the Servo web engine.
 *
 * #ServoGtkWebView is a #GtkWidget subclass. Create one with
 * servo_gtk_web_view_new() (or servo_gtk_web_view_new_with_user_content_manager()
 * to attach a #ServoGtkUserContentManager), add it to a container, and drive it
 * with servo_gtk_web_view_load_url() / servo_gtk_web_view_load_html().
 *
 * Observable state is exposed through the read-only #ServoGtkWebView:uri,
 * #ServoGtkWebView:title and #ServoGtkWebView:is-loading properties (each with a
 * `notify::` signal) and the #ServoGtkWebView::load-changed signal. Page zoom is
 * controlled through the read-write #ServoGtkWebView:zoom-level property.
 *
 * Signals:
 *
 * - #ServoGtkWebView::load-changed `(ServoGtkLoadEvent event)`: emitted when the
 *   page load state changes.
 * - #ServoGtkWebView::create-web-view `(const char *url)`: emitted when page
 *   content requests a new top-level web view (`window.open` / `target=_blank`).
 *   The handler is expected to open a new window hosting a fresh
 *   #ServoGtkWebView loading @url.
 */
G_DECLARE_FINAL_TYPE (ServoGtkWebView, servo_gtk_web_view, SERVO_GTK, WEB_VIEW, GtkWidget)

/**
 * servo_gtk_web_view_new:
 *
 * Creates a new #ServoGtkWebView.
 *
 * Returns: (transfer full): a new #ServoGtkWebView, as a #GtkWidget
 */
GtkWidget *servo_gtk_web_view_new (void);

/**
 * servo_gtk_web_view_new_with_user_content_manager:
 * @manager: (transfer none): a #ServoGtkUserContentManager
 *
 * Creates a new #ServoGtkWebView with @manager attached, enabling
 * user-script/style injection and the page-to-native message channel.
 *
 * Returns: (transfer full): a new #ServoGtkWebView, as a #GtkWidget
 */
GtkWidget *servo_gtk_web_view_new_with_user_content_manager (ServoGtkUserContentManager *manager);

/**
 * servo_gtk_web_view_load_url:
 * @self: a #ServoGtkWebView
 * @url: the URL to load
 *
 * Loads @url in the web view.
 */
void servo_gtk_web_view_load_url (ServoGtkWebView *self,
                                  const char      *url);

/**
 * servo_gtk_web_view_load_html:
 * @self: a #ServoGtkWebView
 * @html: an HTML document as a string
 * @base_url: (nullable): a base URL, accepted for API parity (see note)
 *
 * Loads an in-memory HTML document.
 *
 * The current implementation loads @html through a `data:` URL, so relative
 * links are resolved against that data URL rather than @base_url.
 */
void servo_gtk_web_view_load_html (ServoGtkWebView *self,
                                   const char      *html,
                                   const char      *base_url);

/**
 * servo_gtk_web_view_evaluate_javascript:
 * @self: a #ServoGtkWebView
 * @script: the JavaScript source to evaluate
 *
 * Evaluates a snippet of JavaScript in the current page. Fire-and-forget: the
 * result is not returned.
 */
void servo_gtk_web_view_evaluate_javascript (ServoGtkWebView *self,
                                              const char      *script);

/**
 * servo_gtk_web_view_reload:
 * @self: a #ServoGtkWebView
 *
 * Reloads the current page.
 */
void servo_gtk_web_view_reload (ServoGtkWebView *self);

/**
 * servo_gtk_web_view_go_back:
 * @self: a #ServoGtkWebView
 *
 * Navigates back in session history.
 */
void servo_gtk_web_view_go_back (ServoGtkWebView *self);

/**
 * servo_gtk_web_view_go_forward:
 * @self: a #ServoGtkWebView
 *
 * Navigates forward in session history.
 */
void servo_gtk_web_view_go_forward (ServoGtkWebView *self);

/**
 * servo_gtk_web_view_get_uri:
 * @self: a #ServoGtkWebView
 *
 * Gets the URI of the currently loaded page.
 *
 * Returns: (transfer full) (nullable): the current URI, or %NULL if nothing is
 *   loaded. Free with g_free().
 */
char *servo_gtk_web_view_get_uri (ServoGtkWebView *self);

/**
 * servo_gtk_web_view_get_title:
 * @self: a #ServoGtkWebView
 *
 * Gets the title of the currently loaded page.
 *
 * Returns: (transfer full) (nullable): the current title, or %NULL. Free with
 *   g_free().
 */
char *servo_gtk_web_view_get_title (ServoGtkWebView *self);

/**
 * servo_gtk_web_view_get_is_loading:
 * @self: a #ServoGtkWebView
 *
 * Returns: %TRUE if the view is currently loading a page.
 */
gboolean servo_gtk_web_view_get_is_loading (ServoGtkWebView *self);

/**
 * servo_gtk_web_view_get_zoom_level:
 * @self: a #ServoGtkWebView
 *
 * Returns: the current page zoom level (1.0 == unzoomed).
 */
double servo_gtk_web_view_get_zoom_level (ServoGtkWebView *self);

/**
 * servo_gtk_web_view_set_zoom_level:
 * @self: a #ServoGtkWebView
 * @zoom_level: the desired zoom level (clamped to the accepted range)
 *
 * Sets the page zoom level.
 */
void servo_gtk_web_view_set_zoom_level (ServoGtkWebView *self,
                                        double           zoom_level);

G_END_DECLS

#endif /* SERVO_GTK_WEB_VIEW_H */
