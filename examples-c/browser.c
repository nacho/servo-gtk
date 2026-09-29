/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

/* A C browser built on the servo-gtk C/GObject API, mirroring the Rust
 * `browser` example feature for feature:
 *
 *   - back / forward / reload buttons
 *   - an address bar that loads on Enter and stays in sync with the page URI
 *   - zoom out / reset / in buttons
 *   - a button that loads an in-memory HTML document exercising the script
 *     dialogs (alert/confirm/prompt), a file chooser and a popup link
 *   - a spinner shown while a load is in progress
 *   - the window title reflecting the page title
 *   - popups (window.open / target=_blank) opening a fresh browser window
 *
 * It also demonstrates the required startup sequence for C consumers. */

#include <servo-gtk/servo-gtk.h>

static const char *SAMPLE_HTML =
    "<!DOCTYPE html>"
    "<html><head><meta charset=\"utf-8\"><title>Inline HTML</title></head>"
    "<body style=\"font-family: sans-serif; padding: 2rem;\">"
    "<h1>Hello from load_html()</h1>"
    "<p>This document was loaded from an in-memory string, "
    "not fetched over the network.</p>"
    "<p>"
    "<button onclick=\"alert('This is an alert dialog.')\">alert()</button>"
    "<button onclick=\"document.getElementById('o').textContent = "
    "confirm('Do you confirm?')\">confirm()</button>"
    "<button onclick=\"document.getElementById('o').textContent = "
    "prompt('Type something:', 'default text')\">prompt()</button>"
    "</p>"
    "<p><input type=\"file\" onchange=\"document.getElementById('o').textContent = "
    "this.value\"> (file chooser)</p>"
    "<p><a href=\"https://example.org\" target=\"_blank\">Open example.org in a new window</a></p>"
    "<p>Result: <span id=\"o\"></span></p>"
    "</body></html>";

static void open_browser_window (GtkApplication *app,
                                 const char     *url);

/* Per-window context passed to signal handlers that need more than the
 * web view. Freed when the window is destroyed. */
typedef struct
{
  GtkApplication  *app;
  ServoGtkWebView *web_view;
  GtkWindow       *window;
  GtkEntry        *entry;
  GtkSpinner      *spinner;
} BrowserWindow;

static void
on_entry_activate (GtkEntry *entry,
                   gpointer  user_data)
{
  BrowserWindow *bw = user_data;
  servo_gtk_web_view_load_url (bw->web_view, gtk_editable_get_text (GTK_EDITABLE (entry)));
}

static void
on_reload_clicked (GtkButton *button,
                   gpointer   user_data)
{
  (void) button;
  servo_gtk_web_view_reload (((BrowserWindow *) user_data)->web_view);
}

static void
on_back_clicked (GtkButton *button,
                 gpointer   user_data)
{
  (void) button;
  servo_gtk_web_view_go_back (((BrowserWindow *) user_data)->web_view);
}

static void
on_forward_clicked (GtkButton *button,
                    gpointer   user_data)
{
  (void) button;
  servo_gtk_web_view_go_forward (((BrowserWindow *) user_data)->web_view);
}

static void
on_zoom_out_clicked (GtkButton *button,
                     gpointer   user_data)
{
  (void) button;
  ServoGtkWebView *wv = ((BrowserWindow *) user_data)->web_view;
  servo_gtk_web_view_set_zoom_level (wv, servo_gtk_web_view_get_zoom_level (wv) / 1.2);
}

static void
on_zoom_reset_clicked (GtkButton *button,
                       gpointer   user_data)
{
  (void) button;
  servo_gtk_web_view_set_zoom_level (((BrowserWindow *) user_data)->web_view, 1.0);
}

static void
on_zoom_in_clicked (GtkButton *button,
                    gpointer   user_data)
{
  (void) button;
  ServoGtkWebView *wv = ((BrowserWindow *) user_data)->web_view;
  servo_gtk_web_view_set_zoom_level (wv, servo_gtk_web_view_get_zoom_level (wv) * 1.2);
}

static void
on_html_clicked (GtkButton *button,
                 gpointer   user_data)
{
  (void) button;
  servo_gtk_web_view_load_html (((BrowserWindow *) user_data)->web_view, SAMPLE_HTML, NULL);
}

/* Keep the URL entry in sync with the actual page URI (e.g. after redirects). */
static void
on_uri_notify (GObject    *object,
               GParamSpec *pspec,
               gpointer    user_data)
{
  (void) pspec;
  BrowserWindow *bw = user_data;
  char *uri = servo_gtk_web_view_get_uri (SERVO_GTK_WEB_VIEW (object));
  if (uri != NULL)
    {
      gtk_editable_set_text (GTK_EDITABLE (bw->entry), uri);
      g_free (uri);
    }
}

/* Reflect the page title in the window title. */
static void
on_title_notify (GObject    *object,
                 GParamSpec *pspec,
                 gpointer    user_data)
{
  (void) pspec;
  BrowserWindow *bw = user_data;
  char *title = servo_gtk_web_view_get_title (SERVO_GTK_WEB_VIEW (object));
  if (title != NULL && *title != '\0')
    {
      char *full = g_strdup_printf ("%s \xe2\x80\x94 Servo GTK Browser", title);
      gtk_window_set_title (bw->window, full);
      g_free (full);
    }
  else
    {
      gtk_window_set_title (bw->window, "Servo GTK Browser");
    }
  g_free (title);
}

/* Show a spinner while a load is in progress. */
static void
on_load_changed (ServoGtkWebView   *web_view,
                 ServoGtkLoadEvent  event,
                 gpointer           user_data)
{
  (void) web_view;
  BrowserWindow *bw = user_data;
  switch (event)
    {
    case SERVO_GTK_LOAD_EVENT_STARTED:
      gtk_spinner_start (bw->spinner);
      break;
    case SERVO_GTK_LOAD_EVENT_FINISHED:
      gtk_spinner_stop (bw->spinner);
      break;
    default:
      break;
    }
}

/* Popups (window.open / target=_blank): open a fresh browser window. */
static void
on_create_web_view (ServoGtkWebView *web_view,
                    const char      *url,
                    gpointer         user_data)
{
  (void) web_view;
  BrowserWindow *bw = user_data;
  g_message ("Opening popup window for %s", url);
  open_browser_window (bw->app, url);
}

static GtkWidget *
icon_button (const char *icon_name,
             const char *tooltip)
{
  GtkWidget *button = gtk_button_new_from_icon_name (icon_name);
  gtk_widget_set_tooltip_text (button, tooltip);
  return button;
}

static void
open_browser_window (GtkApplication *app,
                     const char     *url)
{
  BrowserWindow *bw = g_new0 (BrowserWindow, 1);
  bw->app = app;

  GtkWidget *window = gtk_application_window_new (app);
  bw->window = GTK_WINDOW (window);
  gtk_window_set_title (bw->window, "Servo GTK Browser");
  gtk_window_set_default_size (bw->window, 1024, 768);

  GtkWidget *vbox = gtk_box_new (GTK_ORIENTATION_VERTICAL, 5);
  GtkWidget *hbox = gtk_box_new (GTK_ORIENTATION_HORIZONTAL, 5);

  GtkWidget *back_button = icon_button ("go-previous", "Go Back");
  GtkWidget *forward_button = icon_button ("go-next", "Go Forward");
  GtkWidget *reload_button = icon_button ("view-refresh", "Reload");

  GtkWidget *entry = gtk_entry_new ();
  bw->entry = GTK_ENTRY (entry);
  gtk_entry_set_placeholder_text (GTK_ENTRY (entry), "Enter URL...");
  gtk_editable_set_text (GTK_EDITABLE (entry), url);
  gtk_widget_set_hexpand (entry, TRUE);

  GtkWidget *zoom_out_button = icon_button ("zoom-out-symbolic", "Zoom out");
  GtkWidget *zoom_reset_button = icon_button ("zoom-original-symbolic", "Reset zoom");
  GtkWidget *zoom_in_button = icon_button ("zoom-in-symbolic", "Zoom in");
  GtkWidget *html_button =
      icon_button ("text-x-generic-symbolic", "Load a sample inline HTML document");

  GtkWidget *spinner = gtk_spinner_new ();
  bw->spinner = GTK_SPINNER (spinner);
  gtk_widget_set_tooltip_text (spinner, "Loading");

  GtkWidget *web_view = servo_gtk_web_view_new ();
  bw->web_view = SERVO_GTK_WEB_VIEW (web_view);
  gtk_widget_set_hexpand (web_view, TRUE);
  gtk_widget_set_vexpand (web_view, TRUE);

  /* Free the per-window context when the window goes away. */
  g_object_set_data_full (G_OBJECT (window), "browser-window-ctx", bw, g_free);

  g_signal_connect (entry, "activate", G_CALLBACK (on_entry_activate), bw);
  g_signal_connect (reload_button, "clicked", G_CALLBACK (on_reload_clicked), bw);
  g_signal_connect (back_button, "clicked", G_CALLBACK (on_back_clicked), bw);
  g_signal_connect (forward_button, "clicked", G_CALLBACK (on_forward_clicked), bw);
  g_signal_connect (zoom_out_button, "clicked", G_CALLBACK (on_zoom_out_clicked), bw);
  g_signal_connect (zoom_reset_button, "clicked", G_CALLBACK (on_zoom_reset_clicked), bw);
  g_signal_connect (zoom_in_button, "clicked", G_CALLBACK (on_zoom_in_clicked), bw);
  g_signal_connect (html_button, "clicked", G_CALLBACK (on_html_clicked), bw);

  g_signal_connect (web_view, "notify::uri", G_CALLBACK (on_uri_notify), bw);
  g_signal_connect (web_view, "notify::title", G_CALLBACK (on_title_notify), bw);
  g_signal_connect (web_view, "load-changed", G_CALLBACK (on_load_changed), bw);
  g_signal_connect (web_view, "create-web-view", G_CALLBACK (on_create_web_view), bw);

  gtk_box_append (GTK_BOX (hbox), back_button);
  gtk_box_append (GTK_BOX (hbox), forward_button);
  gtk_box_append (GTK_BOX (hbox), reload_button);
  gtk_box_append (GTK_BOX (hbox), entry);
  gtk_box_append (GTK_BOX (hbox), zoom_out_button);
  gtk_box_append (GTK_BOX (hbox), zoom_reset_button);
  gtk_box_append (GTK_BOX (hbox), zoom_in_button);
  gtk_box_append (GTK_BOX (hbox), html_button);
  gtk_box_append (GTK_BOX (hbox), spinner);
  gtk_box_append (GTK_BOX (vbox), hbox);
  gtk_box_append (GTK_BOX (vbox), web_view);

  gtk_window_set_child (bw->window, vbox);
  gtk_window_present (bw->window);

  servo_gtk_web_view_load_url (bw->web_view, url);
}

static void
on_activate (GtkApplication *app,
             gpointer        user_data)
{
  (void) user_data;
  open_browser_window (app, "https://example.com");
}

int
main (int    argc,
      char **argv)
{
  /* MUST be the very first statement: hands off to the Servo runner subprocess
   * when this process was spawned as one. */
  servo_gtk_run_as_runner_if_requested ();

  /* Initialize the GL loader and GTK used by the Servo-backed widget. */
  if (!servo_gtk_init ())
    {
      g_printerr ("failed to initialize servo-gtk\n");
      return 1;
    }

  (void) argc;
  (void) argv;

  GtkApplication *app = gtk_application_new ("com.example.ServoGtkC",
                                             G_APPLICATION_NON_UNIQUE);
  g_signal_connect (app, "activate", G_CALLBACK (on_activate), NULL);

  int status = g_application_run (G_APPLICATION (app), 0, NULL);
  g_object_unref (app);
  return status;
}
