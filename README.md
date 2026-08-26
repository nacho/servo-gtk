# Servo GTK

A GTK4 library that embeds the Servo web engine.

## Features

- GTK4-based web browser widget
- Servo web engine integration
- OpenGL-accelerated rendering
- Async event handling

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

## Dependencies

- GTK4
- OpenGL
- Servo web engine
- Rust toolchain
