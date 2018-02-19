extern crate winit;

use winit::{ControlFlow, Event, EventsLoop, Window, WindowEvent};

fn main() {
    let mut events_loop = EventsLoop::new();
    let window = Window::new(&events_loop).unwrap();
    window.set_title("Hello, World!");
    window.set_inner_size(640, 480);

    events_loop.run_forever(|event| {
        match event {
            Event::WindowEvent { event: WindowEvent::Closed, .. } => {
                ControlFlow::Break
            },
            _ => ControlFlow::Continue,
        }
    });
}
