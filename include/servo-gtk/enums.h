/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#ifndef SERVO_GTK_ENUMS_H
#define SERVO_GTK_ENUMS_H

#include <glib-object.h>

G_BEGIN_DECLS

/**
 * ServoGtkLoadEvent:
 * @SERVO_GTK_LOAD_EVENT_STARTED: a new load request has started.
 * @SERVO_GTK_LOAD_EVENT_FINISHED: loading has finished (all resources loaded,
 *   `document.readyState` == `complete`).
 *
 * The state of a page load, delivered by the #ServoGtkWebView::load-changed
 * signal.
 *
 * Servo only surfaces load start and load completion, so only the corresponding
 * two states are represented here. Observe #ServoGtkWebView:uri changes via
 * `notify::uri` for finer-grained navigation events such as redirects.
 */
typedef enum
{
  SERVO_GTK_LOAD_EVENT_STARTED,
  SERVO_GTK_LOAD_EVENT_FINISHED
} ServoGtkLoadEvent;

#define SERVO_GTK_TYPE_LOAD_EVENT (servo_gtk_load_event_get_type ())

/**
 * servo_gtk_load_event_get_type:
 *
 * Returns: the #GType of the #ServoGtkLoadEvent enumeration.
 */
GType servo_gtk_load_event_get_type (void);

G_END_DECLS

#endif /* SERVO_GTK_ENUMS_H */
