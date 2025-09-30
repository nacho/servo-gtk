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

Then use in your code:

```rust
use servo_gtk::WebView;

let webview = WebView::new();
webview.load_url("https://example.com");
```

## Dependencies

- GTK4
- OpenGL
- Servo web engine
- Rust toolchain
