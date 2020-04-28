mod wow;
use wow::Application;

pub use http_types::Result;

pub fn new()-> Application{
    Application{
        name:"init_name".to_owned()
    }
}