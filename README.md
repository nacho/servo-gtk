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

## Metrics

servo-gtk has built-in, zero-cost-when-off observability. Metrics cover both
processes (the GTK UI process and the Servo runner subprocess) and are merged
into one JSON report written by the UI process.

### Enabling

| Variable | Meaning |
| --- | --- |
| `SERVO_GTK_METRICS=path` | Enable metrics and write the JSON report to `path` (atomically). |
| `SERVO_GTK_METRICS=stderr` (or `1`) | Enable metrics and print the report to stderr. |
| `SERVO_GTK_METRICS_INTERVAL=secs` | Periodic report interval (default `5`; `0` disables). A final report is also written when the `WebView` is disposed. |

Unset, empty or `0` disables metrics; every call site then costs a single
atomic load.

Programmatically: `servo_gtk::metrics::set_enabled(true)` before creating the
`WebView`, then `webview.flush_metrics(|report| ...)` and
`webview.reset_metrics()`.

### Report layout

```json
{
  "schema": 1,
  "pid": 1234,
  "ui":     { "uptime_s": 12.3, "counters": { "ui.frames": 600 },
              "histograms": { "ui.frame.interval_us":
                { "count": 600, "sum": 0, "mean": 0.0, "min": 0, "p50": 0, "p90": 0, "p95": 0, "p99": 0, "max": 0 } } },
  "runner": { "uptime_s": 12.1, "counters": {}, "histograms": {} }
}
```

Histograms ending in `_us` are durations in microseconds. `runner` is `null`
if no runner report was received.

### Metrics

Runner process (`runner.*`):

| Name | Meaning |
| --- | --- |
| `runner.frame.paint_us` | `webview.paint()` |
| `runner.frame.present_us` | present / buffer swap |
| `runner.frame.readback_us` | GL readback (`read_to_image`) |
| `runner.frame.encode_us` | protobuf encoding of `FrameReady` |
| `runner.frame.write_us` | pipe write, including back-pressure |
| `runner.frame.total_us` | whole frame-ready notification, start to end |
| `runner.frame.bytes` | encoded frame size |
| `runner.frame.interval_us` | time between consecutive frame notifications |
| `runner.frames` | counter: frames sent |
| `runner.loop.iteration_us` | one whole event-loop iteration |
| `runner.loop.wait_us` | time blocked waiting for actions |
| `runner.loop.spin_us` | `servo.spin_event_loop()` |
| `runner.loop.actions_us` | processing the drained actions |
| `runner.loop.actions_per_iter` | actions handled per iteration (after coalescing, when > 0) |
| `runner.loop.iterations` | counter: loop iterations |
| `runner.actions.received` / `runner.actions.coalesced` | counters: actions received / merged away |
| `runner.log_messages` | counter: log messages forwarded |

UI process (`ui.*`):

| Name | Meaning |
| --- | --- |
| `ui.ipc.read_body_us` | reading a message body after its length prefix |
| `ui.ipc.decode_us` | protobuf decode |
| `ui.ipc.frame_bytes` | frame message size |
| `ui.ipc.action_queue_us` | `send_action()` until written to the pipe (UI-to-runner back-pressure) |
| `ui.ipc.events` / `ui.ipc.actions` | counters: events received / actions sent |
| `ui.frame.transit_us` | runner send start until fully received in the UI |
| `ui.frame.runner_to_ui_us` | runner paint start until received in the UI |
| `ui.frame.texture_us` | creating the GDK texture from a frame |
| `ui.frame.paint_to_snapshot_us` | runner paint start until the texture is first drawn |
| `ui.frame.interval_us` | time between frame arrivals |
| `ui.frames` / `ui.frames.dropped` | counters: frames received / replaced before ever being drawn |
| `ui.input.to_frame_us` | input event sent until a frame reflecting it arrives |
| `ui.input.to_frame_oldest_us` | same, for the oldest pending input of each frame |
| `ui.inputs` | counter: stamped input events |
| `ui.load.request_to_start_us` | `load_url`/`load_html`/`reload` until load start |
| `ui.load.request_to_end_us` | API request until load end |
| `ui.load.start_to_end_us` | load start until load end |
| `ui.load.end_to_first_frame_us` | load end until the first frame |
| `ui.loads` | counter: page loads |

Keep the window visible while measuring: the compositor throttles drawing for
occluded windows, which shows up as a high `ui.frames.dropped` and few
`ui.frame.paint_to_snapshot_us` samples (the runner-side and input metrics are
unaffected).

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
