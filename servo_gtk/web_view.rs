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

fn logical_to_device(value: f64, scale: f64) -> f64 {
    value * if scale > 0.0 { scale } else { 1.0 }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, glib::Enum)]
#[enum_type(name = "ServoGtkLoadEvent")]
pub enum LoadEvent {
    #[default]
    Started,
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
        pub user_content_manager: RefCell<Option<UserContentManager>>,

        #[property(get, name = "uri", nullable)]
        pub uri: RefCell<Option<String>>,
        #[property(get, name = "title", nullable)]
        pub title: RefCell<Option<String>>,
        #[property(get, name = "is-loading")]
        pub is_loading: Cell<bool>,
        #[property(get, set = Self::set_zoom_level, name = "zoom-level", minimum = 0.1, maximum = 10.0, default = 1.0)]
        pub zoom_level: Cell<f64>,

        /// Counter for consecutive load-starts without a completed load. A
        /// page that fires load-starts in a tight loop (e.g. because an ad
        /// blocker cancelled a resource that page JS retries) is broken by
        /// refusing to process further load-starts beyond a small threshold.
        pub load_loop_guard: Cell<u32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WebView {
        const NAME: &'static str = "ServoGtkWebView";
        type Type = super::WebView;
        type ParentType = gtk::Widget;
    }

    impl WebView {
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
                    Signal::builder("load-changed")
                        .param_types([LoadEvent::static_type()])
                        .build(),
                    Signal::builder("create-web-view")
                        .param_types([String::static_type()])
                        .build(),
                    Signal::builder("adblock-blocked")
                        .param_types([String::static_type(), String::static_type()])
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

    pub fn with_user_content_manager(user_content_manager: &UserContentManager) -> Self {
        let web_view: Self = glib::Object::builder().build();
        if let Some(runner) = web_view.imp().servo_runner.borrow().as_ref() {
            user_content_manager.attach(runner.clone());
        }
        web_view
            .imp()
            .user_content_manager
            .replace(Some(user_content_manager.clone()));
        web_view
    }

    pub fn user_content_manager(&self) -> Option<UserContentManager> {
        self.imp().user_content_manager.borrow().clone()
    }

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

    pub fn load_html(&self, html: &str, base_url: Option<&str>) {
        let imp = self.imp();
        if let Some(servo) = imp.servo_runner.borrow().as_ref() {
            servo.load_html(html, base_url);
        }
    }

    pub fn ping(&self) {
        let imp = self.imp();
        if let Some(servo) = imp.servo_runner.borrow().as_ref() {
            servo.ping();
        }
    }

    pub fn update_adblock_rules(&self, action: &str, rules: Vec<String>) {
        let imp = self.imp();
        if let Some(servo) = imp.servo_runner.borrow().as_ref() {
            servo.update_adblock_rules(action, rules);
        }
    }

    pub fn set_adblock_level(&self, level: i32) {
        let imp = self.imp();
        if let Some(servo) = imp.servo_runner.borrow().as_ref() {
            servo.set_adblock_level(level);
        }
    }

    /// Gửi blob adblock đã serialize xuống runner subprocess.
    ///
    /// Blob là kết quả của `adblock::Engine::serialize_raw()` (xem Kestrel
    /// `adblock_rules` module). Cách này nhanh hơn `update_adblock_rules()`
    /// vì runner chỉ cần deserialize (~200ms) thay vì parse 180k rules
    /// (~5-15s).
    pub fn set_adblock_blob(&self, blob: Vec<u8>) {
        let imp = self.imp();
        if let Some(servo) = imp.servo_runner.borrow().as_ref() {
            servo.set_adblock_blob(blob);
        }
    }

    pub fn connect_adblock_blocked<F: Fn(&Self, &str, &str) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "adblock-blocked",
            false,
            glib::closure_local!(move |obj: &Self, url: String, resource_type: String| {
                f(obj, &url, &resource_type);
            }),
        )
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

    fn show_script_dialog(&self, request: ScriptDialogRequest) {
        let kind = ScriptDialogKind::try_from(request.kind).unwrap_or(ScriptDialogKind::Alert);
        let request_id = request.request_id;
        let parent = gtk::prelude::WidgetExt::root(self).and_downcast::<gtk::Window>();

        match kind {
            ScriptDialogKind::Alert => {
                self.show_alert_dialog(request_id, &request.message);
            }
            ScriptDialogKind::Confirm => {
                let dialog = gtk::AlertDialog::builder()
                    .message(&request.message)
                    .modal(true)
                    .build();
                dialog.set_buttons(&["Cancel", "OK"]);
                dialog.set_cancel_button(0);
                dialog.set_default_button(1);

                let obj_weak = self.downgrade();
                dialog.choose(parent.as_ref(), gio::Cancellable::NONE, move |result| {
                    let confirmed = matches!(result, Ok(1));
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

    fn show_alert_dialog(&self, request_id: u64, message: &str) {
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

        let ok_button = gtk::Button::with_label("OK");
        ok_button.add_css_class("suggested-action");
        ok_button.set_halign(gtk::Align::End);
        content.append(&ok_button);

        window.set_child(Some(&content));

        let responded = Rc::new(Cell::new(false));
        let respond = {
            let obj_weak = self.downgrade();
            let window = window.clone();
            let responded = responded.clone();
            move || {
                if responded.replace(true) {
                    return;
                }
                if let Some(obj) = obj_weak.upgrade()
                    && let Some(servo) = obj.imp().servo_runner.borrow().as_ref()
                {
                    servo.send_script_dialog_response(request_id, true, "");
                }
                window.close();
            }
        };

        let respond_ok = respond.clone();
        ok_button.connect_clicked(move |_| respond_ok());

        let respond_close = respond;
        window.connect_close_request(move |_| {
            respond_close();
            glib::Propagation::Proceed
        });

        window.present();
    }

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

        let respond_close = respond;
        window.connect_close_request(move |_| {
            respond_close(false, String::new());
            glib::Propagation::Proceed
        });

        window.present();
    }

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

        let respond_dismiss = respond.clone();
        popover.connect_closed(move |_| respond_dismiss(false, 0));

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
                let imp = self.imp();
                let count = imp.load_loop_guard.get().saturating_add(1);
                imp.load_loop_guard.set(count);

                if count > 5 {
                    log::warn!(
                        "[kestrel] Breaking navigation loop after {} load-starts (url: {})",
                        count,
                        load_start.url
                    );
                    imp.load_loop_guard.set(0);
                    self.set_is_loading(false);
                    self.emit_load_changed(LoadEvent::Finished);
                    return;
                }

                if !load_start.url.is_empty() {
                    self.set_uri(Some(load_start.url));
                }
                self.set_is_loading(true);
                self.emit_load_changed(LoadEvent::Started);
            }
            servo_event::Event::LoadEnd(load_end) => {
                self.imp().load_loop_guard.set(0);
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
            servo_event::Event::AdblockBlocked(blocked) => {
                self.emit_by_name::<()>("adblock-blocked", &[&blocked.url, &blocked.resource_type]);
            }
        }
    }

    fn set_uri(&self, uri: Option<String>) {
        let imp = self.imp();
        if *imp.uri.borrow() != uri {
            imp.uri.replace(uri);
            self.notify("uri");
        }
    }

    fn set_title(&self, title: Option<String>) {
        let imp = self.imp();
        if *imp.title.borrow() != title {
            imp.title.replace(title);
            self.notify("title");
        }
    }

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
