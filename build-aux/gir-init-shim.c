/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/* Introspection support shim.
 *
 * #ServoGtkWebView is a #GtkWidget subclass implemented in Rust. Its
 * `class_init` asserts that GTK has been initialized on the main thread. The
 * g-ir-scanner introspection *dump* program links this library and refs the
 * widget class to enumerate its properties/signals, but never calls
 * gtk_init() — so without help the dump aborts on that assertion.
 *
 * This library constructor calls gtk_init() when (and only when) the
 * environment variable `SERVO_GTK_GIR_SCAN` is set, which the Meson
 * introspection step does. Normal consumers never set it, so this is a no-op
 * for them and does not force GTK initialization at load time. */

#include <stdlib.h>

#include <glib.h>

/* Implemented in Rust (servo_gtk::ffi). Initializes GTK through gtk4-rs so its
 * main-thread state is recorded — a plain C gtk_init() does not satisfy
 * gtk4-rs's `is_initialized_main_thread()` check. */
extern int servo_gtk_ensure_gtk_init_for_scan (void);

__attribute__((constructor)) static void
servo_gtk_gir_scan_init (void)
{
  if (g_getenv ("SERVO_GTK_GIR_SCAN") != NULL)
    {
      servo_gtk_ensure_gtk_init_for_scan ();
    }
}
