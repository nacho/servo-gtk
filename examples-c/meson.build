# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

# A C browser demonstrating the servo-gtk C/GObject API.
executable(
  'servo-gtk-browser-c',
  'browser.c',
  dependencies: [libservo_gtk_dep, gtk_dep],
  install: false,
)

# Headless smoke test: exercises the type system and the (Servo-free)
# UserContentManager without opening a window. Exits 77 (skip) when no display
# is available.
smoke_test = executable(
  'servo-gtk-smoke-test',
  'smoke-test.c',
  dependencies: [libservo_gtk_dep, gtk_dep],
  install: false,
)

test(
  'c-api-smoke',
  smoke_test,
  # servo_gtk_init() initializes GTK through gtk4-rs, which the Rust-implemented
  # GtkWidget subclass requires before its class can be ref'd. The test skips
  # (exit 77) when no display is available.
  timeout: 120,
)
