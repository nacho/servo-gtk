# servo-gtk (Kestrel fork)

A GTK4 library that embeds the Servo web engine — **fork of
[nacho/servo-gtk](https://github.com/nacho/servo-gtk) with content-blocking support
for the [Kestrel browser](https://github.com/LocShadowVN/kestrel-browser)**.

[![Upstream](https://img.shields.io/badge/upstream-nacho%2Fservo--gtk-blue.svg?style=flat-square)](https://github.com/nacho/servo-gtk)
[![License](https://img.shields.io/badge/license-MPL--2.0-blue.svg?style=flat-square)](LICENSE)
[![Fork for](https://img.shields.io/badge/fork%20for-Kestrel%20browser-orange.svg?style=flat-square)](https://github.com/LocShadowVN/kestrel-browser)

---

> **This is a fork.** The canonical project lives at
> [nacho/servo-gtk](https://github.com/nacho/servo-gtk). This fork exists only
> to add features needed by Kestrel. If you are not building Kestrel, use
> upstream.

---

## About this fork

`servo-gtk` embeds the [Servo](https://servo.org) web engine into a GTK4
widget. This fork adds a **network-level content-blocking hook** on top of the
upstream API, so an embedder can inspect and cancel resource requests before
they are dispatched by Servo.

Everything else — the GTK4 widget, input handling, HiDPI rendering, script
dialogs, file chooser, HTTP auth, permissions, context menus, popups, user
content injection, the Meson build, and the GObject-Introspection bindings —
is unchanged from upstream.

## What this fork adds

- **`AdblockState`** — wrapper around [`adblock-rust`](https://github.com/brave/adblock-rust),
  Brave's filter engine. Loads EasyList-format rules, checks requests, and
  runs entirely inside the Servo runner subprocess (no IPC round-trip per
  request).
- **`WebViewDelegate::load_web_resource`** hook — inspects every resource
  request. Main-frame navigations are never blocked; only subresources
  (scripts, images, stylesheets, fonts, media, XHR, ...) can be cancelled.
- **New GTK signals and methods on `WebView`**:
  - `WebView::update_adblock_rules(action, rules)`
  - `WebView::set_adblock_level(level)` — `0 = Off`, `1 = Standard`, `2 = Aggressive`
  - `WebView::set_adblock_blob(blob)` — fast path, deserializes a pre-built engine
  - `WebView::connect_adblock_blocked(|wv, url, resource_type|)` — fired for
    each blocked resource, useful for a badge counter
- **New IPC messages** in `proto/ipc.proto`:
  `AdblockRulesUpdate`, `SetAdblockLevel`, `SetAdblockBlob`, `AdblockBlocked`,
  `AdblockLevel`.

The adblock engine lives in the runner subprocess alongside Servo, so per-request
checks are a plain function call rather than an IPC round-trip.

## Building

```bash
cargo build
```

## Running the example

```bash
cargo run --example browser
```

## Using as a library

Add to your `Cargo.toml`:

```toml
[dependencies]
servo-gtk = { git = "https://github.com/LocShadowVN/servo-gtk.git", rev = "<commit>" }
```

Then, at the very top of `main()`:

```rust
fn main() {
    servo_gtk::run_as_runner_if_requested();

    // ... your normal application startup ...
}
```

Loading ad-block rules and switching level:

```rust
let rules: Vec<String> = std::fs::read_to_string("easylist.txt")?
    .lines()
    .map(String::from)
    .collect();

webview.update_adblock_rules("load", rules);
webview.set_adblock_level(1); // Standard

webview.connect_adblock_blocked(|_wv, url, resource_type| {
    println!("blocked: {url} ({resource_type})");
});
```

Or, for faster startup, build the engine once and ship a pre-serialized blob:

```rust
let engine = adblock::Engine::from_rules(&rules, Default::default());
let blob = engine.serialize_raw()?;
webview.set_adblock_blob(blob);
```

## Using from C and other languages (GObject Introspection)

In addition to the Rust crate, `servo-gtk` can be built as a C-ABI shared
library (`libservo-gtk.so`) that exposes its GObjects — `ServoGtkWebView` (a
`GtkWidget` subclass) and `ServoGtkUserContentManager` — to C and to any
language with GObject-Introspection bindings (Python, JavaScript, Vala, ...).
The Meson build also produces a pkg-config file (`servo-gtk.pc`) and
introspection data (`ServoGtk-0.1.gir` and `ServoGtk-0.1.typelib`).

### Building with Meson

```bash
meson setup builddir
meson compile -C builddir
meson test -C builddir      # headless C API smoke test
meson install -C builddir   # installs the .so, headers, servo-gtk.pc, .gir and .typelib
```

The Meson build wraps `cargo` to produce a static library and links it into the
shared library, so a Rust toolchain is still required. GObject Introspection
generation is Linux-only and requires `g-ir-scanner` (the `gobject-introspection`
package) and a display (`$DISPLAY`/`$WAYLAND_DISPLAY`); it can be toggled with
`-Dintrospection=enabled|disabled|auto`.

### Using the C API

Include the umbrella header and link with pkg-config:

```c
#include <servo-gtk/servo-gtk.h>

int main(int argc, char **argv) {
    /* MUST be the very first statement (see the runner note above). */
    servo_gtk_run_as_runner_if_requested();
    servo_gtk_init();  /* set up the GL loader */

    /* ... gtk_init(); create a ServoGtkWebView; run your application ... */
    GtkWidget *view = servo_gtk_web_view_new();
    servo_gtk_web_view_load_url(SERVO_GTK_WEB_VIEW(view), "https://example.com");
}
```

```bash
cc $(pkg-config --cflags servo-gtk) browser.c $(pkg-config --libs servo-gtk)
```

A complete C browser lives in [`examples-c/browser.c`](examples-c/browser.c).

### Using from Python (PyGObject)

```python
import gi
gi.require_version("ServoGtk", "0.1")
from gi.repository import ServoGtk

ServoGtk.run_as_runner_if_requested()
ServoGtk.init()
view = ServoGtk.WebView.new()
view.load_url("https://example.com")
```

> Note: because the shared library statically links the entire Servo engine,
> loading it lazily via `dlopen` (as PyGObject does on first use) may emit a
> `cannot allocate memory in static TLS block` warning. The introspection
> metadata still loads correctly, and C consumers that link the library at
> startup are unaffected.

## Requirements

- GTK4
- OpenGL
- Servo web engine
- Rust toolchain
- `protobuf-compiler` (for `prost-build`)
- For the C library / introspection: Meson, pkg-config, and
  `gobject-introspection` (`g-ir-scanner`)

## Credits

Upstream `servo-gtk` is written and maintained by
[Ignacio Casal Quinteiro (nacho)](https://github.com/nacho). The adblock
integration in this fork is a downstream addition; bugs in the adblock layer
should be reported here, bugs in the base widget should be reported upstream.

## License

Mozilla Public License 2.0, same as upstream. See [LICENSE](LICENSE).