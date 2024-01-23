use std::convert::TryFrom;
use std::mem;
use std::num::NonZeroU32;
use std::sync::Arc;

use winit::{
    dpi::{LogicalPosition, PhysicalPosition, Position, Size},
    event::*,
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};

use cgmath::{prelude::*, Point2, Vector2, Vector3, Vector4};
use indextree::NodeId;

use crate::core::engine::Engine;
use wgpu::{util::DeviceExt, SamplerBindingType, TextureView};
use winit::event::VirtualKeyCode::E;
use winit::platform::run_return::EventLoopExtRunReturn;

// Do this before importing local crates.
pub mod asset;
pub mod core;
pub mod math;
pub mod render;
pub mod scene;
pub mod text;
pub mod vector_image;
pub mod window;

// Import local crates.
use crate::asset::AssetServer;
// use crate::render::atlas::{Atlas, AtlasInstance};
// use crate::render::gizmo::Gizmo;
use crate::render::render_world::RenderWorld;
use crate::render::{RenderServer, Texture, TextureId};
use crate::scene::Sprite2d;
// use crate::scene::sprite3d::Sprite3d;
// use crate::scene::vector_sprite::{DrawVector, VectorSprite};
use crate::scene::{AsNode, Camera2d, World};
use crate::text::TextServer;
// use crate::scene::{
//     AsNode, Camera2d, Camera3d, Camera3dController, Label, Light, LightUniform, Model, Projection,
//     Sky, World,
// };
// use crate::text::TextServer;
use crate::window::InputServer;

const INITIAL_WINDOW_WIDTH: u32 = 1280;
const INITIAL_WINDOW_HEIGHT: u32 = 720;

pub struct Singletons {
    pub engine: Engine,
    pub render_server: RenderServer,
    pub input_server: InputServer,
    pub text_server: TextServer,
    pub asset_server: AssetServer,
}

pub struct App {
    window: Window,
    window_size: winit::dpi::PhysicalSize<u32>,
    world: World,
    pub render_world: RenderWorld,
    pub singletons: Singletons,
    is_init: bool,
    /// In order to call EventLoop::run_return from App::run,
    /// we have to put it in an option to avoid borrow errors.
    event_loop: Option<EventLoop<()>>,
}

impl App {
    pub fn new() -> Self {
        let event_loop = EventLoop::new();

        let env = env_logger::Env::default()
            .filter_or("EUREKA_LOG_LEVEL", "info")
            .write_style_or("EUREKA_LOG_STYLE", "always");
        env_logger::init_from_env(env);

        // Use cargo package name as the window title.
        let title = env!("CARGO_PKG_NAME");

        let window_size =
            winit::dpi::PhysicalSize::new(INITIAL_WINDOW_WIDTH, INITIAL_WINDOW_HEIGHT);

        let window = WindowBuilder::new()
            .with_title(title)
            .with_inner_size(window_size)
            .build(&event_loop)
            .unwrap();

        // App::init_render uses async code, so we're going to wait for it to finish.
        let mut render_server = pollster::block_on(App::init_render(&window));

        let mut engine = Engine::new();

        let asset_server = AssetServer::new();

        let mut world = World::new(Vector2::new(window_size.width, window_size.height));

        let mut render_world = RenderWorld::new(&render_server);

        let text_server = TextServer::new(&render_server, &mut render_world.texture_cache);

        let singletons = Singletons {
            engine,
            render_server,
            input_server: InputServer::new(),
            text_server,
            asset_server,
        };

        Self {
            window,
            window_size,
            world,
            render_world,
            singletons,
            is_init: false,
            event_loop: Some(event_loop),
        }
    }

    // Creating some of the wgpu types requires async code.
    async fn init_render(window: &Window) -> RenderServer {
        // Context for all other wgpu objects.
        let instance = wgpu::Instance::default();

        // Handle to a presentable surface.
        let surface = unsafe { instance.create_surface(window).unwrap() };

        // Handle to a physical graphics and/or compute device.
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        // Use the adapter to create a device and a queue.
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    features: wgpu::Features::empty(),
                    limits: wgpu::Limits::default(),
                    label: None,
                },
                None,
            )
            .await
            .unwrap();

        // Get the window's inner size.
        let size = window.inner_size();

        let surface_config = surface
            .get_default_config(&adapter, size.width, size.height)
            .expect("Surface unsupported by adapter!");
        surface.configure(&device, &surface_config);

        // Create a render server.
        RenderServer::new(surface, surface_config, device, queue)
    }

    pub fn run(&mut self) {
        // Main loop.
        self.event_loop.take().unwrap().run_return(|event, _, control_flow| {
            match event {
                // Device event.
                Event::DeviceEvent {
                    ref event,
                    .. // We're not using device_id currently.
                } => {
                    // We're not handling raw input data currently.
                }
                // Window event.
                Event::WindowEvent {
                    ref event,
                    window_id,
                } if window_id == self.window.id() => {
                    match event {
                        WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                        WindowEvent::Resized(physical_size) => {
                            // See https://github.com/rust-windowing/winit/issues/2094.
                            if self.is_init {
                                return;
                            }

                            self.resize(*physical_size);

                            log::info!("Window resized to {:?}", physical_size);
                        }
                        // Scale factor changed.
                        WindowEvent::ScaleFactorChanged { new_inner_size, .. } => {
                            self.resize(**new_inner_size);

                            log::info!("Scale factor changed, new window size is {:?}", new_inner_size);
                        }
                        _ => {
                            // Other input events should be handled by the input server.
                            self.input(event);
                        }
                    }
                }
                // Redraw request.
                Event::RedrawRequested(_) => {
                    self.singletons.input_server.update(&self.window);

                    self.update();

                    match self.render() {
                        Ok(_) => {}
                        // Reconfigure the surface if lost.
                        Err(wgpu::SurfaceError::Lost) => self.resize(self.window_size),
                        // The system is out of memory, we should probably quit.
                        Err(wgpu::SurfaceError::OutOfMemory) => *control_flow = ControlFlow::Exit,
                        // All other errors (Outdated, Timeout) should be resolved by the next frame.
                        Err(e) => eprintln!("App resource error: {:?}", e),
                    }
                }
                Event::MainEventsCleared => {
                    // RedrawRequested will only trigger once, unless we manually request it.
                    self.window.request_redraw();
                }
                Event::NewEvents(cause) => {
                    if cause == StartCause::Init {
                        self.is_init = true;
                    } else {
                        self.is_init = false;
                    }
                }
                _ => {}
            }
        });
    }

    pub fn add_node(&mut self, new_node: Box<dyn AsNode>, parent: Option<NodeId>) {
        self.world.add_node(new_node, parent);
    }

    fn capture_cursor() {}

    fn release_cursor() {}

    /// Resize window.
    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        // Reconfigure the surface everytime the window's size changes.
        if new_size.width > 0 && new_size.height > 0 {
            self.window_size = new_size;

            let config = &mut self.singletons.render_server.surface_config;
            config.width = new_size.width;
            config.height = new_size.height;

            self.singletons
                .render_server
                .surface
                .configure(&self.singletons.render_server.device, config);

            self.render_world
                .recreate_depth_texture(&self.singletons.render_server);

            self.world
                .when_view_size_changes(Vector2::new(new_size.width, new_size.height))
        }
    }

    /// Handle input events.
    fn input(&mut self, event: &WindowEvent) -> bool {
        // Convert to our own input events.
        self.singletons
            .input_server
            .prepare_input_event(&self.window, event);

        self.world.input(&mut self.singletons.input_server);

        true
    }

    fn update(&mut self) {
        self.singletons.engine.tick();

        // self.world
        //     .get_node_mut::<Label>(self.fps_label_id)
        //     .unwrap()
        //     .set_text(format!(
        //         "FPS: {}",
        //         self.singletons.core_server.get_fps() as i32
        //     ));

        self.world.update(
            self.singletons.engine.get_delta() as f32,
            &mut self.singletons,
        );
    }

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        // Collects draw commands from the scene world.
        let draw_commands = self.world.queue_draw();

        // Extract render entities from the draw commands.
        self.render_world.extract(&draw_commands);

        let render_server = &self.singletons.render_server;

        self.render_world.prepare(render_server);

        // Update server GPU resources.
        self.singletons
            .text_server
            .prepare(&self.singletons.render_server, &mut self.render_world.texture_cache);

        let render_world = &self.render_world;

        // First we need to get a frame to draw to.
        let surface_texture = render_server.surface.get_current_texture()?;

        // Creates a TextureView with default settings.
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let depth_texture = render_world
            .texture_cache
            .get(render_world.surface_depth_texture)
            .unwrap();

        // Builds a command buffer that we can then send to the GPU.
        let mut encoder =
            render_server
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("main render encoder"),
                });

        // The RenderPass has all the methods to do the actual drawing.
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main render pass"),
                color_attachments: &[
                    // This is what @location(0) in the fragment shader targets.
                    Some(wgpu::RenderPassColorAttachment {
                        view: &view, // Change this to change where to draw.
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.1,
                                g: 0.2,
                                b: 0.3,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_texture.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.render_world.render(&mut render_pass);
        }

        // Finish the command encoder to generate a command buffer,
        // then submit it for execution.
        self.singletons
            .render_server
            .queue
            .submit(std::iter::once(encoder.finish()));

        // Present the swapchain surface.
        surface_texture.present();

        Ok(())
    }
}
