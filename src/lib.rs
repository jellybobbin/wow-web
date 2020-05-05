pub mod wow;
pub mod context;
pub mod router;

use wow::Application;

pub use http_types::Result;

pub fn new()-> Application{
    Application::new()
}

pub fn default()-> Application{
    Application::new()
}