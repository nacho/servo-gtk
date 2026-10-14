/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/* Headless integration test for the servo-gtk C/GObject API.
 *
 * This links the shared library and exercises the type system without opening a
 * window or starting the Servo engine subprocess (which would require a display
 * and network). It verifies that:
 *   - the runner hand-off returns in a normal (non-runner) process,
 *   - the GObject types register with the expected names and parents,
 *   - the ServoGtkLoadEvent enum values are as declared, and
 *   - the documented signals are registered on the types.
 *
 * Instantiating a #ServoGtkWebView is intentionally avoided here because its
 * construction spawns the Servo runner subprocess. */

#include <servo-gtk/servo-gtk.h>

static void
assert_signal (GType       type,
               const char *name)
{
  /* Ensure the class is initialized so its signals are installed (the Rust
   * `signals()` implementation runs during class_init). */
  gpointer klass = g_type_class_ref (type);
  guint id = g_signal_lookup (name, type);
  if (id == 0)
    {
      g_error ("signal '%s' not found on %s", name, g_type_name (type));
    }
  g_type_class_unref (klass);
}

int
main (int    argc,
      char **argv)
{
  (void) argc;
  (void) argv;

  /* Must return immediately: this process was not spawned as a runner. */
  servo_gtk_run_as_runner_if_requested ();

  /* Initializes the GL loader and GTK (through gtk4-rs, which the Rust-
   * implemented widget's class initialization requires). Skip the test when the
   * environment has no display and GTK cannot be initialized. */
  if (!servo_gtk_init ())
    {
      g_message ("servo_gtk_init failed (no display?); skipping");
      return 77; /* Meson/Automake "skip" exit code */
    }

  /* Types register with the expected names. */
  GType web_view_type = servo_gtk_web_view_get_type ();
  g_assert_cmpstr (g_type_name (web_view_type), ==, "ServoGtkWebView");
  g_assert_true (g_type_is_a (web_view_type, GTK_TYPE_WIDGET));

  GType ucm_type = servo_gtk_user_content_manager_get_type ();
  g_assert_cmpstr (g_type_name (ucm_type), ==, "ServoGtkUserContentManager");
  g_assert_true (g_type_is_a (ucm_type, G_TYPE_OBJECT));

  GType load_event_type = servo_gtk_load_event_get_type ();
  g_assert_cmpstr (g_type_name (load_event_type), ==, "ServoGtkLoadEvent");
  g_assert_true (G_TYPE_IS_ENUM (load_event_type));

  /* Enum values. */
  g_assert_cmpint (SERVO_GTK_LOAD_EVENT_STARTED, ==, 0);
  g_assert_cmpint (SERVO_GTK_LOAD_EVENT_FINISHED, ==, 1);

  /* Documented signals are registered. */
  assert_signal (web_view_type, "load-changed");
  assert_signal (web_view_type, "create-web-view");

  /* A UserContentManager is a plain GObject (no Servo runner), so it is safe to
   * instantiate and exercise here. */
  ServoGtkUserContentManager *ucm = servo_gtk_user_content_manager_new ();
  g_assert_nonnull (ucm);
  assert_signal (ucm_type, "script-message-received");
  servo_gtk_user_content_manager_add_script (ucm, "console.log('hi');");
  servo_gtk_user_content_manager_add_style_sheet (ucm, "body { color: red; }");
  servo_gtk_user_content_manager_register_script_message_handler (ucm, "test");
  servo_gtk_user_content_manager_unregister_script_message_handler (ucm, "test");
  servo_gtk_user_content_manager_remove_all_scripts (ucm);
  servo_gtk_user_content_manager_remove_all_style_sheets (ucm);
  g_object_unref (ucm);

  g_message ("servo-gtk C API smoke test passed");
  return 0;
}
