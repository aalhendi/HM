use core::num::NonZeroU32;
use softbuffer::{Context, Surface};
use std::rc::Rc;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowAttributes, WindowId},
};

#[cfg(not(target_os = "linux"))]
compile_error!("linux_platform can only be built on Linux.");

struct LinuxApp {
    state: LinuxAppState,
}

enum LinuxAppState {
    Uninitialized,
    Running(LinuxState),
}
struct LinuxState {
    window: Rc<Window>,
    _context: Context<Rc<Window>>,
    surface: Surface<Rc<Window>, Rc<Window>>,
}

impl ApplicationHandler for LinuxApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let LinuxAppState::Uninitialized = self.state {
            let window = Rc::new(
                event_loop
                    .create_window(WindowAttributes::default())
                    .expect("Failed to create window"),
            );

            let context =
                Context::new(Rc::clone(&window)).expect("Failed to create softbuffer context");
            let surface = Surface::new(&context, Rc::clone(&window))
                .expect("Failed to create softbuffer surface");

            self.state = LinuxAppState::Running(LinuxState {
                window,
                _context: context,
                surface,
            });
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::ActivationTokenDone { .. } => {}
            WindowEvent::Resized(_) => {}
            WindowEvent::Moved(_) => {}
            WindowEvent::CloseRequested => {
                println!("Close window requested");
                event_loop.exit();
            }
            WindowEvent::Destroyed => {}
            WindowEvent::DroppedFile(_) => {}
            WindowEvent::HoveredFile(_) => {}
            WindowEvent::HoveredFileCancelled => {}
            WindowEvent::Focused(_) => {}
            WindowEvent::KeyboardInput { .. } => {}
            WindowEvent::ModifiersChanged(_) => {}
            WindowEvent::Ime(_) => {}
            WindowEvent::CursorMoved { .. } => {}
            WindowEvent::CursorEntered { .. } => {}
            WindowEvent::CursorLeft { .. } => {}
            WindowEvent::MouseWheel { .. } => {}
            WindowEvent::MouseInput { .. } => {}
            WindowEvent::PinchGesture { .. } => {}
            WindowEvent::PanGesture { .. } => {}
            WindowEvent::DoubleTapGesture { .. } => {}
            WindowEvent::RotationGesture { .. } => {}
            WindowEvent::TouchpadPressure { .. } => {}
            WindowEvent::AxisMotion { .. } => {}
            WindowEvent::Touch(_) => {}
            WindowEvent::ScaleFactorChanged { .. } => {}
            WindowEvent::ThemeChanged(_) => {}
            WindowEvent::Occluded(_) => {}
            WindowEvent::RedrawRequested => {
                let LinuxAppState::Running(state) = &mut self.state else {
                    return;
                };

                let size = state.window.inner_size();
                // NOTE(aalhendi): a minimized/transitioning Wayland window can report zero size.
                // `softbuffer` requires non-zero dimensions, and there is nothing to present.
                let Some(width) = NonZeroU32::new(size.width) else {
                    return;
                };
                let Some(height) = NonZeroU32::new(size.height) else {
                    return;
                };

                state
                    .surface
                    .resize(width, height)
                    .expect("Failed to resize surface");
                let mut buffer = state
                    .surface
                    .buffer_mut()
                    .expect("Failed to get surface buffer");
                for pixel in buffer.iter_mut() {
                    *pixel = 0x00FF00FF;
                }

                buffer.present().expect("Failed to present surface buffer");
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let LinuxAppState::Running(state) = &self.state {
            state.window.request_redraw();
        }
    }
}

pub fn run() {
    let mut app = LinuxApp {
        state: LinuxAppState::Uninitialized,
    };
    let event_loop = EventLoop::new().expect("Failed to create event loop");
    // NOTE(aalhendi): continuously run event loop, even if OS hasn't dispatched any events.
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop
        .run_app(&mut app)
        .expect("Failed to run event loop");
}
