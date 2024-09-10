extern crate winit;

use winit::{dpi::PhysicalSize, event_loop::EventLoop, window::Window};

fn main() {
    let events_loop = EventLoop::new().unwrap();
    let window = Window::new(&events_loop).unwrap();
    window.set_title("Hello, World!");
    window
        .request_inner_size(PhysicalSize::new(640, 480))
        .unwrap();

    events_loop
        .run(move |_event, _control_flow| {
            // TODO: implement control flow to close the window?
            //
        })
        .unwrap();
}
