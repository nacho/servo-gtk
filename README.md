# servo-gtk (Kestrel fork)

A GTK4 library that embeds the Servo web engine — **fork of
[nacho/servo-gtk](https://github.com/nacho/servo-gtk) with ad-blocking support
for the [Kestrel browser](https://github.com/LocShadowVN/kestrel-browser)**.

[![Upstream](https://img.shields.io/badge/upstream-nacho%2Fservo--gtk-blue.svg?style=flat-square)](https://github.com/nacho/servo-gtk)
[![License](https://img.shields.io/badge/license-MPL--2.0-blue.svg?style=flat-square)](LICENSE)
[![Fork for](https://img.shields.io/badge/fork%20for-Kestrel%20browser-orange.svg?style=flat-square)](https://github.com/LocShadowVN/kestrel-browser)

[English](#english) · [Tiếng Việt](#tiếng-việt)

---

> **This is a fork.** The canonical project lives at
> [nacho/servo-gtk](https://github.com/nacho/servo-gtk). This fork exists only
> to add features needed by Kestrel. If you are not building Kestrel, use
> upstream.

---

<a name="english"></a>
## English

### About this fork

`servo-gtk` embeds the [Servo](https://servo.org) web engine into a GTK4
widget. This fork adds a **network-level ad-blocking hook** on top of the
upstream API, so an embedder can inspect and cancel resource requests before
they are dispatched by Servo.

Everything else — the GTK4 widget, input handling, HiDPI rendering, script
dialogs, file chooser, HTTP auth, permissions, context menus, popups, user
content injection — is unchanged from upstream.

### What this fork adds

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
  - `WebView::connect_adblock_blocked(|wv, url, resource_type|)` — fired for
    each blocked resource, useful for a badge counter
- **New IPC messages** in `proto/ipc.proto`:
  `AdblockRulesUpdate`, `SetAdblockLevel`, `AdblockBlocked`, `AdblockLevel`.

The adblock engine lives in the runner subprocess alongside Servo, so per-request
checks are a plain function call rather than an IPC round-trip.

### Building

```bash
cargo build
```

### Running the example

```bash
cargo run --example browser
```

### Using as a library

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

### Requirements

- GTK4
- OpenGL
- Servo web engine
- Rust toolchain
- `protobuf-compiler` (for `prost-build`)

### Credits

Upstream `servo-gtk` is written and maintained by
[Ignacio Casal Quinteiro (nacho)](https://github.com/nacho). The adblock
integration in this fork is a downstream addition; bugs in the adblock layer
should be reported here, bugs in the base widget should be reported upstream.

### License

Mozilla Public License 2.0, same as upstream. See [LICENSE](LICENSE).

---

<a name="tiếng-việt"></a>
## Tiếng Việt

### Về bản fork này

`servo-gtk` nhúng engine trình duyệt [Servo](https://servo.org) vào một widget
GTK4. Bản fork này thêm một **hook chặn quảng cáo ở tầng network** lên trên API
gốc, cho phép ứng dụng nhúng kiểm tra và hủy các request tài nguyên trước khi
Servo gửi chúng đi.

Toàn bộ phần còn lại — widget GTK4, xử lý input, render HiDPI, dialog script,
file chooser, HTTP auth, permission, context menu, popup, inject user content —
giữ nguyên như upstream.

### Bản fork thêm gì

- **`AdblockState`** — wrapper quanh [`adblock-rust`](https://github.com/brave/adblock-rust),
  engine lọc của Brave. Nạp rules định dạng EasyList, kiểm tra request, và chạy
  hoàn toàn bên trong subprocess Servo runner (không cần IPC round-trip cho
  mỗi request).
- **Hook `WebViewDelegate::load_web_resource`** — kiểm tra mọi request tài
  nguyên. Navigation main-frame không bao giờ bị chặn; chỉ các subresource
  (script, image, stylesheet, font, media, XHR, ...) mới có thể bị hủy.
- **Signal và method mới trên `WebView`**:
  - `WebView::update_adblock_rules(action, rules)`
  - `WebView::set_adblock_level(level)` — `0 = Off`, `1 = Standard`, `2 = Aggressive`
  - `WebView::connect_adblock_blocked(|wv, url, resource_type|)` — phát ra cho
    mỗi tài nguyên bị chặn, dùng để hiển thị badge đếm
- **Message IPC mới** trong `proto/ipc.proto`:
  `AdblockRulesUpdate`, `SetAdblockLevel`, `AdblockBlocked`, `AdblockLevel`.

Engine adblock nằm trong subprocess runner cùng với Servo, nên việc kiểm tra
request chỉ là một lời gọi hàm — không phải IPC round-trip.

### Build

```bash
cargo build
```

### Chạy example

```bash
cargo run --example browser
```

### Dùng như thư viện

Thêm vào `Cargo.toml`:

```toml
[dependencies]
servo-gtk = { git = "https://github.com/LocShadowVN/servo-gtk.git", rev = "<commit>" }
```

Rồi ở đầu hàm `main()`:

```rust
fn main() {
    servo_gtk::run_as_runner_if_requested();

    // ... khởi tạo ứng dụng bình thường ...
}
```

Nạp rules và đổi level:

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

### Yêu cầu

- GTK4
- OpenGL
- Servo web engine
- Rust toolchain
- `protobuf-compiler` (cho `prost-build`)

### Ghi công

`servo-gtk` gốc được viết và duy trì bởi
[Ignacio Casal Quinteiro (nacho)](https://github.com/nacho). Phần tích hợp
adblock trong bản fork này là phần mở rộng hạ nguồn; bug trong tầng adblock
xin báo ở đây, bug trong widget gốc xin báo upstream.

### Giấy phép

Mozilla Public License 2.0, giống upstream. Xem [LICENSE](LICENSE).
