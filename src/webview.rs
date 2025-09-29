use crate::servo_runner::{ServoEvent, ServoRunner};
use glib::translate::*;
use glib::{info, warn};
use gtk::gdk;
use gtk::prelude::*;
use gtk::{glib, subclass::prelude::*};
use image::RgbaImage;
use std::cell::RefCell;
use std::ffi::CString;

const G_LOG_DOMAIN: &str = "ServoGtk";

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct WebView {
        pub gl_area: RefCell<Option<gtk::GLArea>>,
        pub servo_runner: RefCell<Option<ServoRunner>>,
        pub last_image: RefCell<Option<RgbaImage>>,
        pub shader_program: RefCell<u32>,
        pub vao: RefCell<u32>,
        pub texture: RefCell<u32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WebView {
        const NAME: &'static str = "WebView";
        type Type = super::WebView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    impl ObjectImpl for WebView {
        fn constructed(&self) {
            self.parent_constructed();

            let gl_area = gtk::GLArea::new();

            let obj_weak = self.obj().downgrade();
            gl_area.connect_realize(move |area| {
                area.make_current();
                gl::load_with(|name| epoxy::get_proc_addr(name) as *const _);

                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    unsafe {
                        // Create shader program
                        let vertex_shader = gl::CreateShader(gl::VERTEX_SHADER);
                        let vertex_source = CString::new(
                            "#version 320 es\n\
                             precision highp float;\n\
                             layout (location = 0) in vec2 aPos;\n\
                             layout (location = 1) in vec2 aTexCoord;\n\
                             out vec2 TexCoord;\n\
                             void main() {\n\
                                 gl_Position = vec4(aPos, 0.0, 1.0);\n\
                                 TexCoord = aTexCoord;\n\
                             }",
                        )
                        .unwrap();
                        gl::ShaderSource(
                            vertex_shader,
                            1,
                            &vertex_source.as_ptr(),
                            std::ptr::null(),
                        );
                        gl::CompileShader(vertex_shader);
                        let mut status: i32 = 0;
                        gl::GetShaderiv(vertex_shader, gl::COMPILE_STATUS, &mut status);
                        warn!("STATUS {status}");

                        let fragment_shader = gl::CreateShader(gl::FRAGMENT_SHADER);
                        let fragment_source = CString::new(
                            "#version 320 es\n\
                             precision highp float;\n\
                             out vec4 FragColor;\n\
                             in vec2 TexCoord;\n\
                             uniform sampler2D ourTexture;\n\
                             void main() {\n\
                                 FragColor = texture(ourTexture, TexCoord);\n\
                             }",
                        )
                        .unwrap();
                        gl::ShaderSource(
                            fragment_shader,
                            1,
                            &fragment_source.as_ptr(),
                            std::ptr::null(),
                        );
                        gl::CompileShader(fragment_shader);
                        let mut status: i32 = 0;
                        gl::GetShaderiv(fragment_shader, gl::COMPILE_STATUS, &mut status);
                        if status == 0 {
                            let mut buf: [u8; 1024] = [0; 1024];
                            gl::GetShaderInfoLog(
                                fragment_shader,
                                1024,
                                std::ptr::null_mut(),
                                buf.as_mut_ptr().cast(),
                            );
                            warn!("INFO LOG {:?}", std::str::from_utf8(&buf[..]).unwrap());
                        }

                        let program = gl::CreateProgram();
                        gl::AttachShader(program, vertex_shader);
                        gl::AttachShader(program, fragment_shader);
                        gl::LinkProgram(program);
                        gl::DeleteShader(vertex_shader);
                        gl::DeleteShader(fragment_shader);

                        imp.shader_program.replace(program);

                        // Create VAO and VBO
                        let vertices: [f32; 16] = [
                            -1.0, -1.0, 0.0, 1.0, // bottom left
                            1.0, -1.0, 1.0, 1.0, // bottom right
                            1.0, 1.0, 1.0, 0.0, // top right
                            -1.0, 1.0, 0.0, 0.0, // top left
                        ];
                        let indices: [u32; 6] = [0, 1, 2, 2, 3, 0];

                        let mut vao = 0;
                        let mut vbo = 0;
                        let mut ebo = 0;
                        gl::GenVertexArrays(1, &mut vao);
                        gl::GenBuffers(1, &mut vbo);
                        gl::GenBuffers(1, &mut ebo);

                        gl::BindVertexArray(vao);
                        gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
                        gl::BufferData(
                            gl::ARRAY_BUFFER,
                            (vertices.len() * std::mem::size_of::<f32>()) as isize,
                            vertices.as_ptr() as *const _,
                            gl::STATIC_DRAW,
                        );

                        gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, ebo);
                        gl::BufferData(
                            gl::ELEMENT_ARRAY_BUFFER,
                            (indices.len() * std::mem::size_of::<u32>()) as isize,
                            indices.as_ptr() as *const _,
                            gl::STATIC_DRAW,
                        );

                        gl::VertexAttribPointer(
                            0,
                            2,
                            gl::FLOAT,
                            gl::FALSE,
                            4 * std::mem::size_of::<f32>() as i32,
                            std::ptr::null(),
                        );
                        gl::EnableVertexAttribArray(0);
                        gl::VertexAttribPointer(
                            1,
                            2,
                            gl::FLOAT,
                            gl::FALSE,
                            4 * std::mem::size_of::<f32>() as i32,
                            (2 * std::mem::size_of::<f32>()) as *const _,
                        );
                        gl::EnableVertexAttribArray(1);

                        imp.vao.replace(vao);

                        // Create texture
                        let mut texture = 0;
                        gl::GenTextures(1, &mut texture);
                        imp.texture.replace(texture);
                    }
                }
            });

            let obj_weak = self.obj().downgrade();
            gl_area.connect_render(move |_, _| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(rgba_image) = imp.last_image.borrow().as_ref() {
                        unsafe {
                            gl::Clear(gl::COLOR_BUFFER_BIT);

                            // Update texture
                            gl::BindTexture(gl::TEXTURE_2D, *imp.texture.borrow());
                            gl::TexImage2D(
                                gl::TEXTURE_2D,
                                0,
                                gl::RGBA as i32,
                                rgba_image.width() as i32,
                                rgba_image.height() as i32,
                                0,
                                gl::RGBA,
                                gl::UNSIGNED_BYTE,
                                rgba_image.as_raw().as_ptr() as *const _,
                            );
                            gl::TexParameteri(
                                gl::TEXTURE_2D,
                                gl::TEXTURE_MIN_FILTER,
                                gl::LINEAR as i32,
                            );
                            gl::TexParameteri(
                                gl::TEXTURE_2D,
                                gl::TEXTURE_MAG_FILTER,
                                gl::LINEAR as i32,
                            );

                            // Render
                            gl::UseProgram(*imp.shader_program.borrow());
                            gl::BindVertexArray(*imp.vao.borrow());
                            gl::DrawElements(gl::TRIANGLES, 6, gl::UNSIGNED_INT, std::ptr::null());
                        }
                    }
                }
                glib::Propagation::Stop
            });

            let obj_weak = self.obj().downgrade();
            gl_area.connect_resize(move |area, _width, _height| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();

                    if let Some(servo) = imp.servo_runner.borrow().as_ref() {
                        servo.resize(area.width() as u32, area.height() as u32);
                    }
                }
            });

            // Add motion event controller
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
            gl_area.add_controller(motion_controller);

            // Add legacy event controller for button events
            let legacy_controller = gtk::EventControllerLegacy::new();
            let obj_weak = self.obj().downgrade();
            legacy_controller.connect_event(move |_, event| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(servo) = imp.servo_runner.borrow().as_ref() {
                        match event.event_type() {
                            gdk::EventType::ButtonPress => {
                                if let Some(button_event) = event.downcast_ref::<gdk::ButtonEvent>()
                                {
                                    if let Some((x, y)) = button_event.position() {
                                        servo.button_press(button_event.button(), x, y);
                                    }
                                }
                            }
                            gdk::EventType::ButtonRelease => {
                                if let Some(button_event) = event.downcast_ref::<gdk::ButtonEvent>()
                                {
                                    if let Some((x, y)) = button_event.position() {
                                        servo.button_release(button_event.button(), x, y);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                glib::Propagation::Proceed
            });
            gl_area.add_controller(legacy_controller);

            // Add key event controller
            let key_controller = gtk::EventControllerKey::new();
            let obj_weak = self.obj().downgrade();
            key_controller.connect_key_pressed(move |_, keyval, _keycode, _state| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(servo) = imp.servo_runner.borrow().as_ref() {
                        if let Some(unicode) = keyval.to_unicode() {
                            servo.key_press(unicode);
                        }
                    }
                }
                glib::Propagation::Proceed
            });
            let obj_weak = self.obj().downgrade();
            key_controller.connect_key_released(move |_, keyval, _keycode, _state| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(servo) = imp.servo_runner.borrow().as_ref() {
                        if let Some(unicode) = keyval.to_unicode() {
                            servo.key_release(unicode);
                        }
                    }
                }
            });
            gl_area.add_controller(key_controller);

            gl_area.set_parent(&*self.obj());
            self.gl_area.replace(Some(gl_area));

            // Initialize Servo thread (no GL needed)
            let servo_runner = ServoRunner::new();

            // Start async event processing
            let event_receiver = servo_runner.event_receiver().clone();
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

            self.servo_runner.replace(Some(servo_runner));

            info!("Webview constructed");
        }

        fn dispose(&self) {
            if let Some(servo) = self.servo_runner.borrow().as_ref() {
                servo.shutdown();
            }
        }
    }

    impl WidgetImpl for WebView {}
}

glib::wrapper! {
    pub struct WebView(ObjectSubclass<imp::WebView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

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

    fn process_servo_event(&self, event: ServoEvent) {
        match event {
            ServoEvent::FrameReady(rgba_image) => {
                let imp = self.imp();

                imp.last_image.replace(Some(rgba_image));

                if let Some(gl_area) = imp.gl_area.borrow().as_ref() {
                    gl_area.queue_draw();
                }
            }
            ServoEvent::LoadComplete => {
                info!("Page load complete");
            }
        }
    }
}
