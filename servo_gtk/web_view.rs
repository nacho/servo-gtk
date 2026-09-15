/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::key_tables::KeyTables;
use crate::proto_ipc::{
    AuthRequest, ContextMenuRequest, FileChooserRequest, PermissionRequest, ScriptDialogKind,
    ScriptDialogRequest, ServoEvent, servo_event,
};
use crate::servo_runner::{LogLevel, ServoRunner};
use crate::user_content::UserContentManager;
use glib::info;
use glib::subclass::Signal;
use glib::translate::*;
use gtk::gdk;
use gtk::prelude::*;
use gtk::{glib, subclass::prelude::*};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::OnceLock;

const G_LOG_DOMAIN: &str = "ServoGtk";

/// Convert a logical widget coordinate or delta to the device pixels Servo
/// renders and receives input in.
///
/// The scale is the display's *fractional* device-pixel ratio (e.g. 1.25),
/// read from the widget's `GdkSurface`. Servo's surface is sized in device
/// pixels, so pointer and scroll values measured in logical units must be
/// multiplied by the same factor. A non-positive scale is treated as 1.0.
fn logical_to_device(value: f64, scale: f64) -> f64 {
    value * if scale > 0.0 { scale } else { 1.0 }
}

/// The state of a page load, delivered by the [`WebView::connect_load_changed`]
/// signal.
///
/// Servo only surfaces load start and load completion, so only the
/// corresponding two states are represented here. Observe [`WebView::uri`]
/// changes via `notify::uri` for finer-grained navigation events such as
/// redirects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, glib::Enum)]
#[enum_type(name = "ServoGtkLoadEvent")]
pub enum LoadEvent {
    /// A new load request has started.
    #[default]
    Started,
    /// Loading has finished (all resources loaded, `document.readyState` ==
    /// `complete`).
    Finished,
}

mod imp {
    use super::*;

    #[derive(glib::Properties, Default)]
    #[properties(wrapper_type = super::WebView)]
    pub struct WebView {
        pub servo_runner: RefCell<Option<Rc<ServoRunner>>>,
        pub memory_texture: RefCell<Option<gdk::MemoryTexture>>,
        pub key_tables: KeyTables,
        /// The user content manager attached to this WebView, if any.
        pub user_content_manager: RefCell<Option<UserContentManager>>,

        /// The URI of the currently loaded page, or `None` if nothing has been
        /// loaded yet.
        #[property(get, name = "uri", nullable)]
        pub uri: RefCell<Option<String>>,
        /// The title of the currently loaded page.
        #[property(get, name = "title", nullable)]
        pub title: RefCell<Option<String>>,
        /// Whether the view is currently loading a page.
        #[property(get, name = "is-loading")]
        pub is_loading: Cell<bool>,
        /// The page zoom level, where 1.0 is unzoomed.
        ///
        /// Mirrored here rather than read back from the runner so the property
        /// is readable and settable at any point, including before the runner
        /// has applied it. The custom setter (see [`super::WebView::set_zoom_level`])
        /// clamps and forwards the value to the runner.
        #[property(get, set = Self::set_zoom_level, name = "zoom-level", minimum = 0.1, maximum = 10.0, default = 1.0)]
        pub zoom_level: Cell<f64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WebView {
        const NAME: &'static str = "WebView";
        type Type = super::WebView;
        type ParentType = gtk::Widget;
    }

    impl WebView {
        /// Property setter for `zoom-level`.
        ///
        /// Clamps the requested level to the range Servo accepts, caches it so
        /// the getter reflects it immediately, and forwards it to the runner.
        /// The runner clamps again defensively.
        fn set_zoom_level(&self, level: f64) {
            let clamped = level.clamp(0.1, 10.0);
            self.zoom_level.set(clamped);
            if let Some(servo) = self.servo_runner.borrow().as_ref() {
                servo.set_zoom_level(clamped);
            }
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for WebView {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    // Emitted when the load state of the page changes.
                    Signal::builder("load-changed")
                        .param_types([LoadEvent::static_type()])
                        .build(),
                    // Emitted when page content requests a new top-level
                    // WebView (window.open / target=_blank). The single string
                    // parameter is the destination URL. A handler is expected
                    // to open a new window hosting a fresh WebView for it.
                    Signal::builder("create-web-view")
                        .param_types([String::static_type()])
                        .build(),
                ]
            })
        }

        fn constructed(&self) {
            self.parent_constructed();

            self.zoom_level.set(1.0);

            let servo_runner = ServoRunner::new();
            let event_receiver = servo_runner.event_receiver();

            self.servo_runner.replace(Some(Rc::new(servo_runner)));

            let obj_weak = self.obj().downgrade();
            glib::spawn_future_local(async move {
                while let Ok(event) = event_receiver.recv().await {
                    if let Some(obj) = obj_weak.upgrade() {
                        obj.process_servo_event(event);
                    } else {
                        break;
                    }
                }
            });

            // Event controllers
            let motion_controller = gtk::EventControllerMotion::new();
            let obj_weak = self.obj().downgrade();
            motion_controller.connect_motion(move |_, x, y| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(servo) = imp.servo_runner.borrow().as_ref() {
                        let scale = obj.device_scale();
                        servo.motion(logical_to_device(x, scale), logical_to_device(y, scale));
                    }
                }
            });
            self.obj().add_controller(motion_controller);

            let legacy_controller = gtk::EventControllerLegacy::new();
            let obj_weak = self.obj().downgrade();
            legacy_controller.connect_event(move |controller, event| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(servo) = imp.servo_runner.borrow().as_ref()
                        && let Some((x, y)) = obj.translate_event_coordinates(event)
                    {
                        let scale = obj.device_scale();
                        let x = logical_to_device(x, scale);
                        let y = logical_to_device(y, scale);
                        match event.event_type() {
                            gdk::EventType::ButtonPress => {
                                if let Some(button_event) = event.downcast_ref::<gdk::ButtonEvent>()
                                {
                                    servo.button_press(button_event.button(), x, y);
                                }
                                controller.widget().expect("Controller widget").grab_focus();
                            }
                            gdk::EventType::ButtonRelease => {
                                if let Some(button_event) = event.downcast_ref::<gdk::ButtonEvent>()
                                {
                                    servo.button_release(button_event.button(), x, y);
                                }
                            }
                            gdk::EventType::TouchBegin => {
                                servo.touch_begin(x, y);
                                controller.widget().expect("Controller widget").grab_focus();
                            }
                            gdk::EventType::TouchUpdate => {
                                servo.touch_update(x, y);
                            }
                            gdk::EventType::TouchEnd => {
                                servo.touch_end(x, y);
                            }
                            gdk::EventType::TouchCancel => {
                                servo.touch_cancel(x, y);
                            }
                            _ => {}
                        }
                    }
                }
                glib::Propagation::Proceed
            });
            self.obj().add_controller(legacy_controller);

            let key_controller = gtk::EventControllerKey::new();
            let obj_weak = self.obj().downgrade();
            key_controller.connect_key_pressed(move |_, keyval, keycode, state| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(servo) = imp.servo_runner.borrow().as_ref()
                        && let Some((key, is_character, location)) =
                            imp.key_tables.key_from_keyval(keyval.into_glib())
                    {
                        info!("Pressed key {key:?} at location {location:?}");
                        servo.key_press(key, is_character, location, keycode, state.bits());
                    }
                }
                glib::Propagation::Proceed
            });
            let obj_weak = self.obj().downgrade();
            key_controller.connect_key_released(move |_, keyval, keycode, state| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(servo) = imp.servo_runner.borrow().as_ref()
                        && let Some((key, is_character, location)) =
                            imp.key_tables.key_from_keyval(keyval.into_glib())
                    {
                        servo.key_release(key, is_character, location, keycode, state.bits());
                    }
                }
            });
            self.obj().add_controller(key_controller);

            let scroll_controller =
                gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
            let obj_weak = self.obj().downgrade();
            scroll_controller.connect_scroll(move |_, delta_x, delta_y| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(servo) = imp.servo_runner.borrow().as_ref() {
                        let scale = obj.device_scale();
                        servo.scroll(
                            logical_to_device(delta_x, scale),
                            logical_to_device(delta_y, scale),
                        );
                    }
                }
                glib::Propagation::Stop
            });
            self.obj().add_controller(scroll_controller);

            // When the widget moves to a monitor with a different scale factor,
            // its logical size is unchanged so `size_allocate` does not fire.
            // Re-sync the Servo surface so it renders at the new device
            // resolution and the page sees the right devicePixelRatio.
            //
            // A fractional scale change (e.g. 1.0 -> 1.25) does not change the
            // integer scale-factor, so we watch the widget's scale-factor
            // *and*, once realized, the GdkSurface's fractional `scale`.
            fn resync_surface(obj: &super::WebView) {
                let imp = obj.imp();
                if let Some(servo) = imp.servo_runner.borrow().as_ref() {
                    let scale = obj.device_scale();
                    let width = gtk::prelude::WidgetExt::width(obj) as f64;
                    let height = gtk::prelude::WidgetExt::height(obj) as f64;
                    servo.set_hidpi_scale_factor(scale as f32);
                    servo.resize(
                        (width * scale).round() as u32,
                        (height * scale).round() as u32,
                    );
                }
            }

            self.obj().connect_scale_factor_notify(resync_surface);

            // Watch the fractional surface scale once the widget is realized.
            self.obj().connect_realize(|obj| {
                if let Some(native) = gtk::prelude::WidgetExt::native(obj)
                    && let Some(surface) = native.surface()
                {
                    let obj_weak = obj.downgrade();
                    surface.connect_scale_notify(move |_| {
                        if let Some(obj) = obj_weak.upgrade() {
                            resync_surface(&obj);
                        }
                    });
                    // Apply the true fractional scale now that it is known.
                    resync_surface(obj);
                }
            });

            self.obj().set_focusable(true);
            info!("Webview constructed");
        }

        fn dispose(&self) {
            if let Some(servo) = self.servo_runner.borrow().as_ref() {
                servo.shutdown();
            }
        }
    }

    impl WidgetImpl for WebView {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            if let Some(texture) = self.memory_texture.borrow().as_ref() {
                let bounds = gtk::graphene::Rect::new(
                    0.0,
                    0.0,
                    gtk::prelude::WidgetExt::width(self.obj().as_ref()) as f32,
                    gtk::prelude::WidgetExt::height(self.obj().as_ref()) as f32,
                );
                snapshot.append_texture(texture, &bounds);
            }
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            if let Some(servo) = self.servo_runner.borrow().as_ref() {
                // Servo's surface is sized in device pixels. Multiply the
                // logical allocation by the display's fractional scale and hand
                // that same factor to Servo as the HiDPI scale, so the page is
                // laid out at the right size rather than upscaled from logical
                // size. Using the fractional scale (e.g. 1.25) rather than the
                // integer scale-factor (which rounds 1.25 up to 2) keeps the
                // page from rendering too large on fractionally scaled displays.
                let scale = self.obj().device_scale();
                servo.set_hidpi_scale_factor(scale as f32);
                servo.resize(
                    ((width.max(0) as f64) * scale).round() as u32,
                    ((height.max(0) as f64) * scale).round() as u32,
                );
            }
        }
    }
}

glib::wrapper! {
    pub struct WebView(ObjectSubclass<imp::WebView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

#[allow(clippy::new_without_default)]
impl WebView {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    /// Create a `WebView` with an attached [`UserContentManager`], enabling
    /// user-script/style injection and the page-to-native message channel.
    ///
    /// Analogous to constructing a `WebKitWebView` with a
    /// `WebKitUserContentManager`. Any scripts, style sheets, or message
    /// handlers added to the manager before or after this call take effect on
    /// the next page load.
    pub fn with_user_content_manager(user_content_manager: &UserContentManager) -> Self {
        let web_view: Self = glib::Object::builder().build();
        // Hand the manager a handle to this WebView's runner so it can forward
        // user-content actions directly, then flush anything buffered before
        // attach. The manager depends only on the runner, not on the WebView.
        if let Some(runner) = web_view.imp().servo_runner.borrow().as_ref() {
            user_content_manager.attach(runner.clone());
        }
        web_view
            .imp()
            .user_content_manager
            .replace(Some(user_content_manager.clone()));
        web_view
    }

    /// The [`UserContentManager`] attached to this `WebView`, if any.
    pub fn user_content_manager(&self) -> Option<UserContentManager> {
        self.imp().user_content_manager.borrow().clone()
    }

    /// Evaluate a snippet of JavaScript in the currently loaded page.
    ///
    /// Fire-and-forget: the result of the evaluation is not currently returned
    /// to the caller. Analogous to `webkit_web_view_evaluate_javascript()`.
    pub fn evaluate_javascript(&self, script: &str) {
        let imp = self.imp();
        if let Some(servo) = imp.servo_runner.borrow().as_ref() {
            servo.evaluate_javascript(script);
        }
    }

    pub fn load_url(&self, url: &str) {
        let imp = self.imp();
        if let Some(servo) = imp.servo_runner.borrow().as_ref() {
            servo.load_url(url);
        }
    }

    /// Load an HTML document from an in-memory string, analogous to
    /// `webkit_web_view_load_html()`.
    ///
    /// `base_url` is accepted for API parity and future use; the current
    /// implementation loads the HTML through a `data:` URL, so relative links
    /// are resolved against that data URL rather than `base_url`.
    pub fn load_html(&self, html: &str, base_url: Option<&str>) {
        let imp = self.imp();
        if let Some(servo) = imp.servo_runner.borrow().as_ref() {
            servo.load_html(html, base_url);
        }
    }

    /// Trigger a request/response round trip over the internal IPC channel.
    ///
    /// Primarily a diagnostic: it proves the bidirectional channel that backs
    /// the delegate features (script dialogs, file chooser, HTTP auth,
    /// permission and context-menu prompts) is wired up. The runner emits a
    /// request which this `WebView` answers automatically; both sides log the
    /// correlation.
    pub fn ping(&self) {
        let imp = self.imp();
        if let Some(servo) = imp.servo_runner.borrow().as_ref() {
            servo.ping();
        }
    }

    pub fn reload(&self) {
        let imp = self.imp();
        if let Some(servo) = imp.servo_runner.borrow().as_ref() {
            servo.reload();
        }
    }

    pub fn go_back(&self) {
        let imp = self.imp();
        if let Some(servo) = imp.servo_runner.borrow().as_ref() {
            servo.go_back();
        }
    }

    pub fn go_forward(&self) {
        let imp = self.imp();
        if let Some(servo) = imp.servo_runner.borrow().as_ref() {
            servo.go_forward();
        }
    }

    /// Connect to the `load-changed` signal, emitted when the load state of the
    /// page changes.
    ///
    /// Note: the read-only `uri`, `title`, and `is-loading` properties each
    /// have a generated getter (`uri()`, `title()`, `is_loading()`) and a
    /// `notify::` signal (`connect_uri_notify`, `connect_title_notify`,
    /// `connect_is_loading_notify`). The read-write `zoom-level` property has
    /// `zoom_level()`/`set_zoom_level()` accessors and `connect_zoom_level_notify`.
    pub fn connect_load_changed<F: Fn(&Self, LoadEvent) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "load-changed",
            false,
            glib::closure_local!(move |obj: &Self, event: LoadEvent| {
                f(obj, event);
            }),
        )
    }

    /// Connect to the `create-web-view` signal, emitted when page content
    /// requests a new top-level web view (`window.open` / `target=_blank`).
    ///
    /// The handler receives the destination URL and is expected to open a new
    /// window hosting a fresh [`WebView`] loading it. Because the engine runs
    /// one view per subprocess, the auxiliary view Servo created is not
    /// rendered here; this signal lets the embedder recreate the popup as a
    /// real, fully functional window.
    pub fn connect_create_web_view<F: Fn(&Self, &str) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "create-web-view",
            false,
            glib::closure_local!(move |obj: &Self, url: String| {
                f(obj, &url);
            }),
        )
    }

    /// The display's fractional device-pixel ratio for this widget.
    ///
    /// GTK's integer `scale_factor()` rounds a fractional display scale (e.g.
    /// 1.25) up to the next integer (2), which would make Servo render far too
    /// large. The widget's `GdkSurface` exposes the true fractional scale via
    /// `scale()`; use it when the widget is realized, falling back to the
    /// integer factor (then 1.0) before a surface exists.
    fn device_scale(&self) -> f64 {
        if let Some(native) = gtk::prelude::WidgetExt::native(self)
            && let Some(surface) = native.surface()
        {
            let scale = surface.scale();
            if scale > 0.0 {
                return scale;
            }
        }
        (self.scale_factor().max(1)) as f64
    }

    fn translate_event_coordinates(&self, event: &gdk::Event) -> Option<(f64, f64)> {
        let root = gtk::prelude::WidgetExt::root(self)?;
        let native = root.native()?;
        let (nx, ny) = native.surface_transform();

        let (event_x, event_y) = event.position()?;
        let event_x = event_x - nx;
        let event_y = event_y - ny;

        let point = gtk::graphene::Point::new(event_x as f32, event_y as f32);
        let translated = root.compute_point(self, &point)?;

        Some((translated.x() as f64, translated.y() as f64))
    }

    /// Present a script-initiated dialog (alert/confirm/prompt) and send the
    /// user's answer back to the runner over the bidirectional IPC channel.
    ///
    /// Alert and confirm use `gtk::AlertDialog`; prompt uses a small modal
    /// window with a text entry, since `AlertDialog` has no input field.
    fn show_script_dialog(&self, request: ScriptDialogRequest) {
        let kind = ScriptDialogKind::try_from(request.kind).unwrap_or(ScriptDialogKind::Alert);
        let request_id = request.request_id;
        let parent = gtk::prelude::WidgetExt::root(self).and_downcast::<gtk::Window>();

        match kind {
            ScriptDialogKind::Alert | ScriptDialogKind::Confirm => {
                let is_confirm = matches!(kind, ScriptDialogKind::Confirm);
                let dialog = gtk::AlertDialog::builder()
                    .message(&request.message)
                    .modal(true)
                    .build();
                if is_confirm {
                    dialog.set_buttons(&["Cancel", "OK"]);
                    dialog.set_cancel_button(0);
                    dialog.set_default_button(1);
                } else {
                    dialog.set_buttons(&["OK"]);
                    dialog.set_default_button(0);
                }

                let obj_weak = self.downgrade();
                dialog.choose(parent.as_ref(), gio::Cancellable::NONE, move |result| {
                    // For alert the only button (index 0) is a confirm. For
                    // confirm, index 1 is OK; anything else (Cancel, Escape,
                    // error) is a dismissal.
                    let confirmed = if is_confirm {
                        matches!(result, Ok(1))
                    } else {
                        true
                    };
                    if let Some(obj) = obj_weak.upgrade()
                        && let Some(servo) = obj.imp().servo_runner.borrow().as_ref()
                    {
                        servo.send_script_dialog_response(request_id, confirmed, "");
                    }
                });
            }
            ScriptDialogKind::Prompt => {
                self.show_prompt_dialog(request_id, &request.message, &request.default_value);
            }
        }
    }

    /// Present a `prompt()` dialog: a modal window with a message, a text entry
    /// pre-filled with `default_value`, and Cancel/OK buttons.
    fn show_prompt_dialog(&self, request_id: u64, message: &str, default_value: &str) {
        let parent = gtk::prelude::WidgetExt::root(self).and_downcast::<gtk::Window>();
        let window = gtk::Window::builder()
            .title("")
            .modal(true)
            .resizable(false)
            .build();
        if let Some(parent) = parent.as_ref() {
            window.set_transient_for(Some(parent));
        }

        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);

        let label = gtk::Label::new(Some(message));
        label.set_wrap(true);
        label.set_xalign(0.0);
        content.append(&label);

        let entry = gtk::Entry::new();
        entry.set_text(default_value);
        entry.set_activates_default(true);
        content.append(&entry);

        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        buttons.set_halign(gtk::Align::End);
        let cancel_button = gtk::Button::with_label("Cancel");
        let ok_button = gtk::Button::with_label("OK");
        ok_button.add_css_class("suggested-action");
        buttons.append(&cancel_button);
        buttons.append(&ok_button);
        content.append(&buttons);

        window.set_child(Some(&content));

        // A single-fire responder shared by both buttons and the close request,
        // so exactly one response is ever sent for a given dialog.
        let responded = Rc::new(Cell::new(false));
        let respond = {
            let obj_weak = self.downgrade();
            let window = window.clone();
            let responded = responded.clone();
            move |confirmed: bool, value: String| {
                if responded.replace(true) {
                    return;
                }
                if let Some(obj) = obj_weak.upgrade()
                    && let Some(servo) = obj.imp().servo_runner.borrow().as_ref()
                {
                    servo.send_script_dialog_response(request_id, confirmed, &value);
                }
                window.close();
            }
        };

        let respond_cancel = respond.clone();
        cancel_button.connect_clicked(move |_| respond_cancel(false, String::new()));

        let entry_clone = entry.clone();
        let respond_ok = respond.clone();
        ok_button.connect_clicked(move |_| respond_ok(true, entry_clone.text().to_string()));

        // Closing the window (Escape / title bar) counts as a cancel.
        let respond_close = respond;
        window.connect_close_request(move |_| {
            respond_close(false, String::new());
            glib::Propagation::Proceed
        });

        window.present();
    }

    /// Present a native file chooser for a `<input type=file>` element and send
    /// the selected path(s) back to the runner. Cancelling yields an empty
    /// selection.
    fn show_file_chooser(&self, request: FileChooserRequest) {
        let request_id = request.request_id;
        let parent = gtk::prelude::WidgetExt::root(self).and_downcast::<gtk::Window>();
        let dialog = gtk::FileDialog::builder().modal(true).build();

        let obj_weak = self.downgrade();
        let reply = move |paths: Vec<String>| {
            if let Some(obj) = obj_weak.upgrade()
                && let Some(servo) = obj.imp().servo_runner.borrow().as_ref()
            {
                servo.send_file_chooser_response(request_id, paths);
            }
        };

        if request.allow_multiple {
            dialog.open_multiple(parent.as_ref(), gio::Cancellable::NONE, move |result| {
                let mut paths = Vec::new();
                if let Ok(files) = result {
                    for i in 0..files.n_items() {
                        if let Some(file) = files.item(i).and_downcast::<gio::File>()
                            && let Some(path) = file.path()
                        {
                            paths.push(path.to_string_lossy().into_owned());
                        }
                    }
                }
                reply(paths);
            });
        } else {
            dialog.open(parent.as_ref(), gio::Cancellable::NONE, move |result| {
                let mut paths = Vec::new();
                if let Ok(file) = result
                    && let Some(path) = file.path()
                {
                    paths.push(path.to_string_lossy().into_owned());
                }
                reply(paths);
            });
        }
    }

    /// Present an HTTP authentication prompt (username + password) and reply
    /// with the entered credentials, or a cancellation if dismissed.
    fn show_auth_dialog(&self, request: AuthRequest) {
        let request_id = request.request_id;
        let parent = gtk::prelude::WidgetExt::root(self).and_downcast::<gtk::Window>();
        let window = gtk::Window::builder()
            .title("Authentication Required")
            .modal(true)
            .resizable(false)
            .build();
        if let Some(parent) = parent.as_ref() {
            window.set_transient_for(Some(parent));
        }

        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(12);
        content.set_margin_bottom(12);
        content.set_margin_start(12);
        content.set_margin_end(12);

        let kind = if request.for_proxy { "proxy" } else { "server" };
        let label = gtk::Label::new(Some(&format!(
            "The {kind} at {} requires a username and password.",
            request.url
        )));
        label.set_wrap(true);
        label.set_xalign(0.0);
        content.append(&label);

        let user_entry = gtk::Entry::builder().placeholder_text("Username").build();
        let pass_entry = gtk::Entry::builder()
            .placeholder_text("Password")
            .visibility(false)
            .activates_default(true)
            .build();
        content.append(&user_entry);
        content.append(&pass_entry);

        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        buttons.set_halign(gtk::Align::End);
        let cancel_button = gtk::Button::with_label("Cancel");
        let ok_button = gtk::Button::with_label("Authenticate");
        ok_button.add_css_class("suggested-action");
        buttons.append(&cancel_button);
        buttons.append(&ok_button);
        content.append(&buttons);

        window.set_child(Some(&content));

        let responded = Rc::new(Cell::new(false));
        let respond = {
            let obj_weak = self.downgrade();
            let window = window.clone();
            let responded = responded.clone();
            move |confirmed: bool, username: String, password: String| {
                if responded.replace(true) {
                    return;
                }
                if let Some(obj) = obj_weak.upgrade()
                    && let Some(servo) = obj.imp().servo_runner.borrow().as_ref()
                {
                    servo.send_auth_response(request_id, confirmed, &username, &password);
                }
                window.close();
            }
        };

        let respond_cancel = respond.clone();
        cancel_button.connect_clicked(move |_| respond_cancel(false, String::new(), String::new()));

        let (user_clone, pass_clone) = (user_entry.clone(), pass_entry.clone());
        let respond_ok = respond.clone();
        ok_button.connect_clicked(move |_| {
            respond_ok(
                true,
                user_clone.text().to_string(),
                pass_clone.text().to_string(),
            )
        });

        let respond_close = respond;
        window.connect_close_request(move |_| {
            respond_close(false, String::new(), String::new());
            glib::Propagation::Proceed
        });

        window.present();
    }

    /// Present an allow/deny permission prompt and reply with the decision.
    fn show_permission_dialog(&self, request: PermissionRequest) {
        let request_id = request.request_id;
        let parent = gtk::prelude::WidgetExt::root(self).and_downcast::<gtk::Window>();
        let dialog = gtk::AlertDialog::builder()
            .message(format!(
                "This page is requesting permission to use: {}",
                request.feature_name
            ))
            .modal(true)
            .build();
        dialog.set_buttons(&["Deny", "Allow"]);
        dialog.set_cancel_button(0);
        dialog.set_default_button(1);

        let obj_weak = self.downgrade();
        dialog.choose(parent.as_ref(), gio::Cancellable::NONE, move |result| {
            let allow = matches!(result, Ok(1));
            if let Some(obj) = obj_weak.upgrade()
                && let Some(servo) = obj.imp().servo_runner.borrow().as_ref()
            {
                servo.send_permission_response(request_id, allow);
            }
        });
    }

    /// Present a native context menu for web content (e.g. on right-click) and
    /// reply with the chosen entry index, or a dismissal.
    ///
    /// Uses a `gtk::Popover` of buttons anchored at the triggering position.
    /// The runner reports the position in device pixels, so it is converted
    /// back to logical coordinates for placement.
    fn show_context_menu(&self, request: ContextMenuRequest) {
        let request_id = request.request_id;
        let popover = gtk::Popover::builder()
            .autohide(true)
            .has_arrow(false)
            .build();
        popover.set_parent(self);

        let scale = self.scale_factor().max(1) as f64;
        let rect = gtk::gdk::Rectangle::new(
            (request.x as f64 / scale) as i32,
            (request.y as f64 / scale) as i32,
            1,
            1,
        );
        popover.set_pointing_to(Some(&rect));

        let menu_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        popover.set_child(Some(&menu_box));

        // Single-fire responder: exactly one response per menu, whether a
        // button was clicked or the popover was dismissed.
        let responded = Rc::new(Cell::new(false));
        let respond = {
            let obj_weak = self.downgrade();
            let popover = popover.clone();
            let responded = responded.clone();
            move |selected: bool, index: u32| {
                if responded.replace(true) {
                    return;
                }
                if let Some(obj) = obj_weak.upgrade()
                    && let Some(servo) = obj.imp().servo_runner.borrow().as_ref()
                {
                    servo.send_context_menu_response(request_id, selected, index);
                }
                popover.popdown();
            }
        };

        for (index, entry) in request.items.iter().enumerate() {
            if entry.separator {
                menu_box.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
                continue;
            }
            let button = gtk::Button::builder()
                .label(&entry.label)
                .sensitive(entry.enabled)
                .has_frame(false)
                .build();
            let respond_item = respond.clone();
            button.connect_clicked(move |_| respond_item(true, index as u32));
            menu_box.append(&button);
        }

        // Closing the popover without a selection is a dismissal.
        let respond_dismiss = respond.clone();
        popover.connect_closed(move |_| respond_dismiss(false, 0));

        // Unparent the popover once it is closed so it does not leak.
        let popover_cleanup = popover.clone();
        popover.connect_closed(move |_| {
            popover_cleanup.unparent();
        });

        popover.popup();
    }

    fn process_servo_event(&self, event: ServoEvent) {
        let Some(event_type) = event.event else {
            return;
        };

        match event_type {
            servo_event::Event::FrameReady(frame_ready) => {
                let rgba_image = RgbaImage::from_raw(
                    frame_ready.width,
                    frame_ready.height,
                    frame_ready.rgba_data,
                )
                .unwrap();

                let imp = self.imp();

                let bytes = glib::Bytes::from(&rgba_image.as_raw()[..]);
                let texture = gdk::MemoryTexture::new(
                    rgba_image.width() as i32,
                    rgba_image.height() as i32,
                    gdk::MemoryFormat::R8g8b8a8,
                    &bytes,
                    (rgba_image.width() * 4) as usize,
                );

                imp.memory_texture.replace(Some(texture));
                self.queue_draw();
            }
            servo_event::Event::CursorChanged(cursor_changed) => {
                let gdk_cursor = gdk::Cursor::from_name(&cursor_changed.cursor, None);
                if let Some(cursor) = gdk_cursor {
                    gtk::prelude::WidgetExt::set_cursor(self, Some(&cursor));
                }
            }
            servo_event::Event::UrlChanged(url_changed) => {
                self.set_uri(Some(url_changed.url));
            }
            servo_event::Event::TitleChanged(title_changed) => {
                self.set_title(Some(title_changed.title));
            }
            servo_event::Event::LoadStart(load_start) => {
                if !load_start.url.is_empty() {
                    self.set_uri(Some(load_start.url));
                }
                self.set_is_loading(true);
                self.emit_load_changed(LoadEvent::Started);
            }
            servo_event::Event::LoadEnd(load_end) => {
                if !load_end.url.is_empty() {
                    self.set_uri(Some(load_end.url));
                }
                self.set_is_loading(false);
                self.emit_load_changed(LoadEvent::Finished);
            }
            servo_event::Event::LogMessage(log_msg) => {
                if let Some(servo_runner) = self.imp().servo_runner.borrow().as_ref() {
                    servo_runner
                        .handle_log_message(LogLevel::from(log_msg.level), &log_msg.message);
                }
            }
            servo_event::Event::ScriptMessage(script_message) => {
                if let Some(ucm) = self.imp().user_content_manager.borrow().as_ref() {
                    ucm.emit_script_message(&script_message.name, &script_message.body);
                }
            }
            servo_event::Event::PingRequest(ping_request) => {
                // Round-trip test of the bidirectional IPC channel: immediately
                // answer the runner's request with the same id. Real delegate
                // features (dialogs, file chooser, ...) show UI here and reply
                // when the user acts.
                info!(
                    "Received PingRequest id {}, replying",
                    ping_request.request_id
                );
                if let Some(servo) = self.imp().servo_runner.borrow().as_ref() {
                    servo.send_ping_response(ping_request.request_id, true);
                }
            }
            servo_event::Event::ScriptDialogRequest(request) => {
                self.show_script_dialog(request);
            }
            servo_event::Event::FileChooserRequest(request) => {
                self.show_file_chooser(request);
            }
            servo_event::Event::AuthRequest(request) => {
                self.show_auth_dialog(request);
            }
            servo_event::Event::PermissionRequest(request) => {
                self.show_permission_dialog(request);
            }
            servo_event::Event::CreateWebView(create) => {
                self.emit_by_name::<()>("create-web-view", &[&create.url]);
            }
            servo_event::Event::ContextMenuRequest(request) => {
                self.show_context_menu(request);
            }
        }
    }

    /// Update the cached `uri` and fire `notify::uri` if it changed.
    fn set_uri(&self, uri: Option<String>) {
        let imp = self.imp();
        if *imp.uri.borrow() != uri {
            imp.uri.replace(uri);
            self.notify("uri");
        }
    }

    /// Update the cached `title` and fire `notify::title` if it changed.
    fn set_title(&self, title: Option<String>) {
        let imp = self.imp();
        if *imp.title.borrow() != title {
            imp.title.replace(title);
            self.notify("title");
        }
    }

    /// Update the cached `is-loading` and fire `notify::is-loading` if it
    /// changed.
    fn set_is_loading(&self, is_loading: bool) {
        let imp = self.imp();
        if imp.is_loading.get() != is_loading {
            imp.is_loading.set(is_loading);
            self.notify("is-loading");
        }
    }

    fn emit_load_changed(&self, event: LoadEvent) {
        self.emit_by_name::<()>("load-changed", &[&event]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_event_registers_glib_enum_type() {
        // Registering the enum type must succeed and be stable across calls.
        let ty = LoadEvent::static_type();
        assert_eq!(ty, LoadEvent::static_type());
        assert_eq!(ty.name(), "ServoGtkLoadEvent");
    }

    #[test]
    fn load_event_roundtrips_through_value() {
        for event in [LoadEvent::Started, LoadEvent::Finished] {
            let value = event.to_value();
            let recovered = value.get::<LoadEvent>().expect("value holds a LoadEvent");
            assert_eq!(event, recovered);
        }
    }

    #[test]
    fn logical_to_device_scales_by_factor() {
        assert_eq!(logical_to_device(100.0, 1.0), 100.0);
        assert_eq!(logical_to_device(100.0, 2.0), 200.0);
        assert_eq!(logical_to_device(100.0, 1.25), 125.0);
        assert_eq!(logical_to_device(80.0, 1.5), 120.0);
    }

    #[test]
    fn logical_to_device_treats_non_positive_scale_as_one() {
        assert_eq!(logical_to_device(100.0, 0.0), 100.0);
        assert_eq!(logical_to_device(100.0, -1.0), 100.0);
    }
}
