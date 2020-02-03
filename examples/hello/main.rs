extern crate winit;

use winit::{
    event_loop::{ControlFlow, EventLoop}, event::{
        Event, WindowEvent
    },
    window::{
        Window,
    },
    dpi::PhysicalSize,
};

fn main() {
    let mut events_loop = EventLoop::new();
    let window = Window::new(&events_loop).unwrap();
    window.set_title("Hello, World!");
    window.set_inner_size(PhysicalSize::new(640, 480));

    events_loop.run(move |event, _, control_flow| {
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                *control_flow = ControlFlow::Exit
            },
            _ => {
                *control_flow = ControlFlow::Wait
            }
        }
    });
}
