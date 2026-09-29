/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

#ifndef SERVO_GTK_H
#define SERVO_GTK_H

#include <glib.h>

#include "enums.h"
#include "usercontentmanager.h"
#include "webview.h"

G_BEGIN_DECLS

/**
 * servo_gtk_run_as_runner_if_requested:
 *
 * Hands off to the Servo runner subprocess if this process was spawned as one.
 *
 * A C `main()` MUST call this as its very first statement. When the process was
 * launched as the runner subprocess this runs the runner and terminates the
 * process without returning; otherwise it returns immediately and normal
 * application startup can proceed.
 *
 * The library runs Servo in a subprocess by re-executing the host binary, so no
 * separate runner binary needs to be installed.
 */
void servo_gtk_run_as_runner_if_requested (void);

/**
 * servo_gtk_init:
 *
 * Initializes servo-gtk for use from C: sets up the OpenGL function loader used
 * by the Servo-backed #ServoGtkWebView, and initializes GTK.
 *
 * Call this once, on the main thread, after
 * servo_gtk_run_as_runner_if_requested() and before creating a
 * #ServoGtkWebView. It loads the system `libepoxy` and wires it into the GL
 * loader, and initializes GTK so that constructing the Rust-implemented
 * #ServoGtkWebView widget succeeds. It is safe to use together with
 * #GtkApplication (GTK initialization is idempotent).
 *
 * Returns: %TRUE on success, %FALSE if the GL loader or GTK could not be set up.
 */
gboolean servo_gtk_init (void);

G_END_DECLS

#endif /* SERVO_GTK_H */
