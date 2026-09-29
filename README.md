# Servo GTK

A GTK4 library that embeds the Servo web engine.

## Features

- GTK4-based web browser widget
- Servo web engine integration
- OpenGL-accelerated rendering
- Async event handling
- HiDPI rendering at device resolution (correct `window.devicePixelRatio`)
- Page zoom (`zoom-level` property; Ctrl+/Ctrl- style)
- Navigation: `load_url`, `load_html`, reload, back/forward
- Observable `uri`, `title`, `is-loading` properties and a `load-changed` signal
- Full keyboard mapping, including function keys and Insert
- User script/style injection and a page-to-native message channel
- Script dialogs (`alert`, `confirm`, `prompt`) with native GTK dialogs
- `<input type=file>` backed by a native file chooser
- HTTP authentication and permission prompts
- Popups (`window.open` / `target=_blank`) via a `create-web-view` signal
- Native context menus on web content

## Building

```bash
cargo build
```

## Running the Example

```bash
cargo run --example browser
```

## Using as a Library

Add to your `Cargo.toml`:

```toml
[dependencies]
servo-gtk = { path = "path/to/servo-gtk" }
```

Then use in your code. You **must** call `run_as_runner_if_requested()` as the
very first thing in `main()`. The library runs Servo in a subprocess by
re-executing your own binary; this call hands off to the Servo runner when the
process was spawned as one, and returns immediately otherwise. No separate
binary needs to be installed.

```rust
use servo_gtk::WebView;

fn main() {
    servo_gtk::run_as_runner_if_requested();

    // ... your normal application startup ...
    let webview = WebView::new();
    webview.load_url("https://example.com");
}
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

## Dependencies

- GTK4
- OpenGL
- Servo web engine
- Rust toolchain
- For the C library / introspection: Meson, pkg-config, and
  `gobject-introspection` (`g-ir-scanner`)
