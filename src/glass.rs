use std::fmt::Formatter;

use image::ImageError;
use indexmap::IndexMap;
use wgpu::{
    Adapter, CreateSurfaceError, Device, Instance, PowerPreference, Queue, RequestDeviceError,
    SurfaceConfiguration,
};
use winit::{
    error::OsError,
    event::{ElementState, Event, VirtualKeyCode, WindowEvent},
    event_loop::{EventLoop, EventLoopWindowTarget},
    window::{Fullscreen, Window, WindowId},
};

use crate::{
    device_context::{DeviceConfig, DeviceContext},
    window::{
        get_best_videomode, get_centered_window_position, get_fitting_videomode, GlassWindow,
        WindowConfig, WindowPos,
    },
    GlassApp, RenderData,
};

/// [`Glass`] is an application that exposes an easy to use API to organize your winit applications
/// which render using wgpu. Just impl [`GlassApp`] for your application (of any type) and you
/// are good to go.
pub struct Glass<A> {
    app: A,
    config: GlassConfig,
}

impl<A: GlassApp + 'static> Glass<A> {
    pub fn new(app: A, config: GlassConfig) -> Glass<A> {
        Glass {
            app,
            config,
        }
    }

    pub fn run(mut self) -> Result<(), GlassError> {
        let event_loop = EventLoop::new();
        let mut context = GlassContext::new(&event_loop, self.config.clone())?;
        self.app.start(&event_loop, &mut context);
        let mut remove_windows = vec![];
        let mut request_window_close = false;

        event_loop.run(move |event, event_loop, control_flow| {
            control_flow.set_poll();

            // Run input fn
            self.app.input(&mut context, event_loop, &event);
            match event {
                Event::WindowEvent {
                    window_id,
                    event: window_event,
                    ..
                } => {
                    if let Some(window) = context.windows.get_mut(&window_id) {
                        match window_event {
                            WindowEvent::Resized(physical_size) => {
                                // On windows, minimized app can have 0,0 size
                                if physical_size.width > 0 && physical_size.height > 0 {
                                    window.configure_surface_with_size(
                                        context.device_context.device(),
                                        physical_size,
                                    );
                                }
                            }
                            WindowEvent::ScaleFactorChanged {
                                new_inner_size, ..
                            } => {
                                window.configure_surface_with_size(
                                    context.device_context.device(),
                                    *new_inner_size,
                                );
                            }
                            WindowEvent::KeyboardInput {
                                input,
                                is_synthetic,
                                ..
                            } => {
                                if let Some(key) = input.virtual_keycode {
                                    if !is_synthetic
                                        && window.exit_on_esc()
                                        && window.is_focused()
                                        && key == VirtualKeyCode::Escape
                                        && input.state == ElementState::Pressed
                                    {
                                        request_window_close = true;
                                        remove_windows.push(window_id);
                                    }
                                }
                            }
                            WindowEvent::Focused(has_focus) => {
                                window.set_focus(has_focus);
                            }
                            WindowEvent::CloseRequested => {
                                request_window_close = true;
                                remove_windows.push(window_id);
                            }
                            _ => (),
                        }
                    }
                }
                Event::MainEventsCleared => {
                    self.app.update(&mut context);
                    // Close window(s)
                    if request_window_close || context.exit {
                        for window in remove_windows.iter() {
                            context.windows.remove(window);
                        }
                        remove_windows.clear();
                        request_window_close = false;
                        // Exit
                        if context.windows.is_empty() || context.exit {
                            control_flow.set_exit();
                            // Run end
                            self.app.end(&mut context);
                        }
                    }
                    // Render
                    for (_, window) in context.windows.iter() {
                        match window.surface().get_current_texture() {
                            Ok(frame) => {
                                let mut encoder = context
                                    .device_context
                                    .device()
                                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                        label: Some("Render Commands"),
                                    });

                                // Run render & post processing functions
                                self.app.render(&context, RenderData {
                                    encoder: &mut encoder,
                                    window,
                                    frame: &frame,
                                });
                                self.app.post_processing(&context, RenderData {
                                    encoder: &mut encoder,
                                    window,
                                    frame: &frame,
                                });

                                context
                                    .device_context
                                    .queue()
                                    .submit(Some(encoder.finish()));

                                frame.present();

                                self.app.after_render(&context);
                            }
                            Err(error) => {
                                if error == wgpu::SurfaceError::OutOfMemory {
                                    panic!("Swapchain error: {error}. Rendering cannot continue.")
                                }
                            }
                        }
                        window.window().request_redraw();
                    }
                    // End of frame
                    self.app.end_of_frame(&mut context);
                }
                _ => {}
            }
        });
    }
}

/// Configuration of your windows and devices.
#[derive(Debug, Clone)]
pub struct GlassConfig {
    pub device_config: DeviceConfig,
    pub window_configs: Vec<WindowConfig>,
}

impl GlassConfig {
    pub fn windowless() -> Self {
        Self {
            device_config: DeviceConfig::default(),
            window_configs: vec![],
        }
    }

    pub fn performance(width: u32, height: u32) -> Self {
        Self {
            device_config: DeviceConfig {
                power_preference: PowerPreference::HighPerformance,
                ..Default::default()
            },
            window_configs: vec![WindowConfig {
                width,
                height,
                exit_on_esc: false,
                ..WindowConfig::default()
            }],
        }
    }
}

impl Default for GlassConfig {
    fn default() -> Self {
        Self {
            device_config: DeviceConfig::default(),
            window_configs: vec![WindowConfig::default()],
        }
    }
}

#[derive(Debug)]
pub enum GlassError {
    WindowError(OsError),
    SurfaceError(CreateSurfaceError),
    AdapterError,
    DeviceError(RequestDeviceError),
    ImageError(ImageError),
}

impl std::fmt::Display for GlassError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            GlassError::WindowError(e) => format!("WindowError: {}", e),
            GlassError::SurfaceError(e) => format!("SurfaceError: {}", e),
            GlassError::AdapterError => "AdapterError".to_owned(),
            GlassError::DeviceError(e) => format!("DeviceError: {}", e),
            GlassError::ImageError(e) => format!("ImageError: {}", e),
        };
        write!(f, "{}", s)
    }
}

/// The runtime context accessible through [`GlassApp`].
/// You can use the context to create windows at runtime. Or access devices, which are often
/// needed for render or compute functionality.
pub struct GlassContext {
    device_context: DeviceContext,
    windows: IndexMap<WindowId, GlassWindow>,
    exit: bool,
}

impl GlassContext {
    pub fn new(event_loop: &EventLoop<()>, mut config: GlassConfig) -> Result<Self, GlassError> {
        // Create windows from initial configs
        let mut winit_windows = vec![];
        for &window_config in config.window_configs.iter() {
            winit_windows.push((
                window_config,
                Self::create_winit_window(event_loop, &window_config)?,
            ))
        }
        // Modify features & limits needed for common pipelines
        // Add push constants feature for common pipelines
        config.device_config.features |= wgpu::Features::PUSH_CONSTANTS
            | wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
        config.device_config.limits = wgpu::Limits {
            ..config.device_config.limits
        };
        let device_context = DeviceContext::new(
            &config.device_config,
            // Needed to ensure our queue families are compatible with surface
            &winit_windows,
        )?;
        let mut app = Self {
            device_context,
            windows: IndexMap::default(),
            exit: false,
        };
        for (window_config, window) in winit_windows {
            let id = app.add_window(window_config, window)?;
            // Configure window surface with size
            let window = app.windows.get_mut(&id).unwrap();
            window.configure_surface_with_size(
                app.device_context.device(),
                window.window().inner_size(),
            );
        }
        Ok(app)
    }

    #[allow(unused)]
    pub fn instance(&self) -> &Instance {
        self.device_context.instance()
    }

    pub fn adapter(&self) -> &Adapter {
        self.device_context.adapter()
    }

    pub fn device(&self) -> &Device {
        self.device_context.device()
    }

    pub fn queue(&self) -> &Queue {
        self.device_context.queue()
    }

    pub fn configure_surface(&mut self, window_id: &WindowId, config: &SurfaceConfiguration) {
        if let Some(window) = self.windows.get_mut(window_id) {
            window.configure_surface(self.device_context.device(), config);
        } else {
            panic!("No window with id {:?}", window_id);
        }
    }

    pub fn primary_render_window(&self) -> &GlassWindow {
        self.windows.first().unwrap().1
    }

    pub fn primary_render_window_mut(&mut self) -> &mut GlassWindow {
        self.windows.first_mut().unwrap().1
    }

    pub fn render_window(&self, id: WindowId) -> Option<&GlassWindow> {
        self.windows.get(&id)
    }

    pub fn render_window_mut(&mut self, id: WindowId) -> Option<&mut GlassWindow> {
        self.windows.get_mut(&id)
    }

    pub fn create_window(
        &mut self,
        event_loop: &EventLoopWindowTarget<()>,
        config: WindowConfig,
    ) -> Result<WindowId, GlassError> {
        let reconfigure_device = self.windows.is_empty();
        let window = Self::create_winit_window(event_loop, &config)?;
        let id = self.add_window(config, window)?;
        // Reconfigure devices with surface so queue families are correct
        let window = self.windows.get_mut(&id).unwrap();
        if reconfigure_device {
            let surface = window.surface();
            self.device_context.reconfigure_with_surface(surface)?;
        }
        // Configure surface with size
        window.configure_surface_with_size(
            self.device_context.device(),
            window.window().inner_size(),
        );
        Ok(id)
    }

    fn add_window(&mut self, config: WindowConfig, window: Window) -> Result<WindowId, GlassError> {
        let id = window.id();
        let render_window = match GlassWindow::new(&self.device_context, config, window) {
            Ok(window) => window,
            Err(e) => return Err(GlassError::SurfaceError(e)),
        };
        self.windows.insert(id, render_window);
        Ok(id)
    }

    fn create_winit_window(
        event_loop: &EventLoopWindowTarget<()>,
        config: &WindowConfig,
    ) -> Result<Window, GlassError> {
        let mut window_builder = winit::window::WindowBuilder::new()
            .with_inner_size(winit::dpi::LogicalSize::new(config.width, config.height))
            .with_title(config.title);

        // Min size
        if let Some(inner_size) = config.min_size {
            window_builder = window_builder.with_min_inner_size(inner_size);
        }

        // Max size
        if let Some(inner_size) = config.max_size {
            window_builder = window_builder.with_max_inner_size(inner_size);
        }

        window_builder = match &config.pos {
            WindowPos::Maximized => window_builder.with_maximized(true),
            WindowPos::FullScreen => {
                if let Some(monitor) = event_loop.primary_monitor() {
                    window_builder
                        .with_fullscreen(Some(Fullscreen::Exclusive(get_best_videomode(&monitor))))
                } else {
                    window_builder
                }
            }
            WindowPos::SizedFullScreen => {
                if let Some(monitor) = event_loop.primary_monitor() {
                    window_builder.with_fullscreen(Some(Fullscreen::Exclusive(
                        get_fitting_videomode(&monitor, config.width, config.height),
                    )))
                } else {
                    window_builder
                }
            }
            WindowPos::FullScreenBorderless => window_builder
                .with_fullscreen(Some(Fullscreen::Borderless(event_loop.primary_monitor()))),
            WindowPos::Pos(pos) => window_builder.with_position(*pos),
            WindowPos::Centered => {
                if let Some(monitor) = event_loop.primary_monitor() {
                    window_builder.with_position(get_centered_window_position(
                        &monitor,
                        config.width,
                        config.height,
                    ))
                } else {
                    window_builder
                }
            }
        };

        match window_builder.build(event_loop) {
            Ok(w) => Ok(w),
            Err(e) => Err(GlassError::WindowError(e)),
        }
    }

    pub fn exit(&mut self) {
        self.exit = true;
    }
}
