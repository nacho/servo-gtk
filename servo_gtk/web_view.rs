/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::key_tables::KeyTables;
use crate::proto_ipc::{ServoEvent, servo_event};
use crate::servo_runner::{LogLevel, ServoRunner};
use glib::info;
use glib::subclass::Signal;
use glib::translate::*;
use gtk::gdk;
use gtk::prelude::*;
use gtk::{glib, subclass::prelude::*};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::sync::OnceLock;

const G_LOG_DOMAIN: &str = "ServoGtk";

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
        pub servo_runner: RefCell<Option<ServoRunner>>,
        pub memory_texture: RefCell<Option<gdk::MemoryTexture>>,
        pub key_tables: KeyTables,

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
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WebView {
        const NAME: &'static str = "WebView";
        type Type = super::WebView;
        type ParentType = gtk::Widget;
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
                ]
            })
        }

        fn constructed(&self) {
            self.parent_constructed();

            let servo_runner = ServoRunner::new();
            let event_receiver = servo_runner.event_receiver();

            self.servo_runner.replace(Some(servo_runner));

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
                        servo.motion(x, y);
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
                        servo.scroll(delta_x, delta_y);
                    }
                }
                glib::Propagation::Stop
            });
            self.obj().add_controller(scroll_controller);

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
                servo.resize(width as u32, height as u32);
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

    pub fn load_url(&self, url: &str) {
        let imp = self.imp();
        if let Some(servo) = imp.servo_runner.borrow().as_ref() {
            servo.load_url(url);
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
    /// `connect_is_loading_notify`).
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
}
