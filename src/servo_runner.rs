use async_channel;
use dpi::PhysicalSize;
use embedder_traits::resources;
use euclid::Point2D;
use glib::{debug, info, warn};
use image::RgbaImage;
use servo::ServoBuilder;
use servo::webrender_api::units::DeviceIntRect;
use servo::{RenderingContext, SoftwareRenderingContext, WebView, WebViewBuilder, WebViewDelegate};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use url::Url;

use crate::resources::ResourceReaderInstance;

const G_LOG_DOMAIN: &str = "ServoGtk";

pub enum ServoAction {
    LoadUrl(String),
    Resize(u32, u32),
    Shutdown,
}

pub enum ServoEvent {
    FrameReady(RgbaImage),
    LoadComplete,
}

pub struct ServoRunner {
    sender: Sender<ServoAction>,
    event_receiver: async_channel::Receiver<ServoEvent>,
}

struct ServoWebViewDelegate {
    event_sender: async_channel::Sender<ServoEvent>,
    rendering_context: Rc<dyn RenderingContext>,
}

impl ServoWebViewDelegate {
    pub(crate) fn new(
        event_sender: async_channel::Sender<ServoEvent>,
        rendering_context: Rc<dyn RenderingContext>,
    ) -> Self {
        Self {
            event_sender,
            rendering_context,
        }
    }
}

impl WebViewDelegate for ServoWebViewDelegate {
    fn notify_new_frame_ready(&self, webview: WebView) {
        let size = self.rendering_context.size2d().to_i32();
        let viewport_rect = DeviceIntRect::from_origin_and_size(Point2D::origin(), size);
        webview.paint();
        self.rendering_context.present();
        if let Some(rgba_image) = self.rendering_context.read_to_image(viewport_rect) {
            if let Err(e) = self
                .event_sender
                .send_blocking(ServoEvent::FrameReady(rgba_image.clone()))
            {
                warn!("Could not set the pixels: {e}");
            }
        }
    }
}

impl ServoRunner {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        let (event_sender, event_receiver) = async_channel::unbounded();

        thread::spawn(move || {
            Self::run_servo(receiver, event_sender);
        });

        Self {
            sender,
            event_receiver,
        }
    }

    pub fn event_receiver(&self) -> &async_channel::Receiver<ServoEvent> {
        &self.event_receiver
    }

    pub fn load_url(&self, url: &str) {
        let _ = self.sender.send(ServoAction::LoadUrl(url.to_string()));
    }

    pub fn resize(&self, width: u32, height: u32) {
        let _ = self.sender.send(ServoAction::Resize(width, height));
    }

    pub fn shutdown(&self) {
        let _ = self.sender.send(ServoAction::Shutdown);
    }

    fn run_servo(receiver: Receiver<ServoAction>, event_sender: async_channel::Sender<ServoEvent>) {
        info!("Servo thread running");

        init_crypto();
        // FIXME
        let resource_dir =
            PathBuf::from("/home/ANT.AMAZON.COM/qignacio/git/servo-gtk").join("resources");
        debug!("Resources are located at: {:?}", resource_dir);
        resources::set(Box::new(ResourceReaderInstance::new(resource_dir.clone())));

        // Create rendering context with initial size (matching servoshell pattern)
        let size = PhysicalSize::new(800, 600);
        let rendering_context = Rc::new(
            SoftwareRenderingContext::new(size)
                .expect("Failed to create Software rendering context"),
        );

        // Use ServoBuilder pattern like servoshell
        let servo_builder = ServoBuilder::new(rendering_context.clone());

        let servo = servo_builder.build();

        let delegate = ServoWebViewDelegate::new(event_sender, rendering_context);
        let webview = WebViewBuilder::new(&servo)
            .delegate(Rc::new(delegate))
            .build();

        loop {
            if let Ok(command) = receiver.try_recv() {
                match command {
                    ServoAction::LoadUrl(url) => {
                        if let Ok(parsed_url) = Url::parse(&url) {
                            // TODO: Send load URL command to Servo
                            info!("Loading URL: {}", url);
                            webview.load(parsed_url);
                        } else {
                            warn!("Invalid URL");
                        }
                    }
                    ServoAction::Resize(width, height) => {
                        info!("Resizing to: {}x{}", width, height);
                        webview.resize(PhysicalSize::new(width, height));
                    }
                    ServoAction::Shutdown => break,
                }
            }

            if !servo.spin_event_loop() {
                break;
            }
        }
    }
}

fn init_crypto() {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Error initializing crypto provider");
}
