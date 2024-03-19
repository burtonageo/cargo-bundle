extern crate winit;

use winit::{
    dpi::PhysicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::Window,
};

fn main() {
    let events_loop = EventLoop::new().unwrap();
    let window = Window::new(&events_loop).unwrap();
    window.set_title("Hello, World!");
    window.request_inner_size(PhysicalSize::new(640, 480)).unwrap();

    events_loop.run(move |event, control_flow| {
			// TODO: implement control flow to close the window?
			//
		}).unwrap();
}
