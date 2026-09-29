# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

project(
  'servo-gtk',
  'c',
  version: '0.1.0',
  meson_version: '>= 1.0.0',
  default_options: ['c_std=c11', 'warning_level=2'],
)

gnome = import('gnome')
pkgconfig = import('pkgconfig')

glib_req = '>= 2.74'

glib_dep = dependency('glib-2.0', version: glib_req)
gobject_dep = dependency('gobject-2.0', version: glib_req)
gio_dep = dependency('gio-2.0', version: glib_req)
gtk_dep = dependency('gtk4', version: '>= 4.12')

# Native system libraries Servo links against (fontconfig/freetype font backend,
# etc.). `cargo build` resolves these when it links a binary; because we perform
# the final shared-library link ourselves, they must be declared here.
fontconfig_dep = dependency('fontconfig')
freetype_dep = dependency('freetype2')

servo_gtk_deps = [glib_dep, gobject_dep, gio_dep, gtk_dep]

# Extra dependencies needed only for the final native link of the Rust
# staticlib (not part of the public API surface exposed to consumers).
servo_gtk_private_deps = [fontconfig_dep, freetype_dep]

# ---------------------------------------------------------------------------
# Public C headers
# ---------------------------------------------------------------------------

servo_gtk_headers = files(
  'include/servo-gtk/enums.h',
  'include/servo-gtk/servo-gtk.h',
  'include/servo-gtk/usercontentmanager.h',
  'include/servo-gtk/webview.h',
)

servo_gtk_inc = include_directories('include')

install_headers(servo_gtk_headers, subdir: 'servo-gtk')

# ---------------------------------------------------------------------------
# Rust static library (built via cargo)
# ---------------------------------------------------------------------------

cargo_script = find_program('build-aux/cargo.py')

# All Rust sources, so Meson reruns cargo when they change.
rust_sources = files(
  'Cargo.toml',
  'Cargo.lock',
  'build.rs',
  'servo_gtk/ffi/lib.rs',
  'servo_gtk/ffi/user_content_manager.rs',
  'servo_gtk/ffi/web_view.rs',
  'servo_gtk/key_tables.rs',
  'servo_gtk/lib.rs',
  'servo_gtk/proto_ipc.rs',
  'servo_gtk/runner.rs',
  'servo_gtk/servo_runner.rs',
  'servo_gtk/user_content.rs',
  'servo_gtk/web_view.rs',
)

cargo_env = environment()
if get_option('werror')
  cargo_env.append('RUSTFLAGS', '-Dwarnings', separator: ' ')
endif

servo_gtk_ffi = custom_target(
  'servo_gtk_ffi',
  build_by_default: true,
  input: rust_sources,
  output: 'libservo_gtk.a',
  console: true,
  env: cargo_env,
  command: [
    cargo_script,
    'build',
    meson.current_build_dir() / 'cargo-target',
    meson.current_source_dir(),
    get_option('buildtype'),
    'unknown', # features: use crate defaults
    'unknown', # target triple: host default
    '@OUTPUT@',
  ],
)

# ---------------------------------------------------------------------------
# Shared library
# ---------------------------------------------------------------------------

# The Rust artifact is a plain .a produced by a custom_target that bundles the
# entire Servo engine plus its vendored C dependencies. Whole-archiving it is
# infeasible (duplicate vendored symbols, multi-GB debug objects), so we link it
# as a normal archive and:
#   * force the public API members to be pulled in with `-Wl,-u,<symbol>`, and
#   * export only those symbols (hiding everything else) with a version script,
#     which also prevents vendored-symbol collisions from leaking.
servo_gtk_public_symbols = [
  'servo_gtk_init',
  'servo_gtk_run_as_runner_if_requested',
  'servo_gtk_web_view_get_type',
  'servo_gtk_load_event_get_type',
  'servo_gtk_web_view_new',
  'servo_gtk_web_view_new_with_user_content_manager',
  'servo_gtk_web_view_load_url',
  'servo_gtk_web_view_load_html',
  'servo_gtk_web_view_evaluate_javascript',
  'servo_gtk_web_view_reload',
  'servo_gtk_web_view_go_back',
  'servo_gtk_web_view_go_forward',
  'servo_gtk_web_view_get_uri',
  'servo_gtk_web_view_get_title',
  'servo_gtk_web_view_get_is_loading',
  'servo_gtk_web_view_get_zoom_level',
  'servo_gtk_web_view_set_zoom_level',
  'servo_gtk_user_content_manager_get_type',
  'servo_gtk_user_content_manager_new',
  'servo_gtk_user_content_manager_add_script',
  'servo_gtk_user_content_manager_add_style_sheet',
  'servo_gtk_user_content_manager_remove_all_scripts',
  'servo_gtk_user_content_manager_remove_all_style_sheets',
  'servo_gtk_user_content_manager_register_script_message_handler',
  'servo_gtk_user_content_manager_unregister_script_message_handler',
]

servo_gtk_link_args = [
  meson.current_build_dir() / 'libservo_gtk.a',
  '-Wl,--version-script,' + meson.current_source_dir() / 'build-aux/servo-gtk.map',
  # Servo vendors C++ code (fontsan/ots, mozangle) and C code needing the math
  # and zlib libraries; `cargo build` supplies these when it links, but here we
  # do the final link ourselves so they must be named explicitly.
  '-lstdc++',
  '-lm',
  '-lz',
  '-ldl',
  '-lpthread',
]
foreach sym : servo_gtk_public_symbols
  servo_gtk_link_args += '-Wl,-u,' + sym
endforeach

libservo_gtk = shared_library(
  'servo-gtk',
  sources: [servo_gtk_ffi, 'build-aux/gir-init-shim.c'],
  dependencies: servo_gtk_deps + servo_gtk_private_deps,
  include_directories: servo_gtk_inc,
  link_args: servo_gtk_link_args,
  install: true,
)

libservo_gtk_dep = declare_dependency(
  link_with: libservo_gtk,
  include_directories: servo_gtk_inc,
  dependencies: servo_gtk_deps,
)

# ---------------------------------------------------------------------------
# pkg-config
# ---------------------------------------------------------------------------

pkgconfig.generate(
  libraries: [libservo_gtk],
  name: 'servo-gtk',
  description: 'A GTK4 library that embeds the Servo web engine, with a C/GObject API.',
  version: meson.project_version(),
  requires: [
    'glib-2.0 >= 2.74',
    'gobject-2.0 >= 2.74',
    'gio-2.0 >= 2.74',
    'gtk4 >= 4.12',
  ],
  subdirs: 'servo-gtk',
)

# ---------------------------------------------------------------------------
# GObject Introspection (.gir + .typelib)
# ---------------------------------------------------------------------------

gir = find_program('g-ir-scanner', required: get_option('introspection'))

if gir.found() and host_machine.system() == 'linux'
  gnome.generate_gir(
    libservo_gtk,
    sources: servo_gtk_headers,
    namespace: 'ServoGtk',
    nsversion: '0.1',
    identifier_prefix: 'ServoGtk',
    symbol_prefix: 'servo_gtk',
    export_packages: 'servo-gtk',
    includes: ['GLib-2.0', 'GObject-2.0', 'Gio-2.0', 'Gtk-4.0'],
    header: 'servo-gtk/servo-gtk.h',
    dependencies: servo_gtk_deps,
    # Force GTK initialization in the introspection dump program (see
    # build-aux/gir-init-shim.c) so refing the Rust-implemented GtkWidget
    # subclass does not abort on GTK's "not initialized" assertion.
    env: ['SERVO_GTK_GIR_SCAN=1'],
    # Suppress compiler warnings while scanning the headers.
    extra_args: ['--cflags-begin', '-Wp,-w', '--cflags-end'],
    install: true,
  )
endif

subdir('examples-c')
