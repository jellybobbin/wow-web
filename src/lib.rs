mod wow;
mod core;

use wow::Application;

pub use http_types::Result;

pub fn new()-> Application{
    Application::new()
}