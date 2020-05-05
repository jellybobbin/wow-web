use http_types::{Response,Request,Result};
use async_std::future::Future;

type BoxFut = Box<dyn Future<Output = http_types::Result<Response>> + Send>;
pub trait Handle {
    fn handle(&self, ctx: Context) -> BoxFut;
}

impl<F> Handle for F
    where
        F: Fn(Context) -> BoxFut,
{
    fn handle(&self, ctx: Context) -> BoxFut {
        (*self)(ctx)
    }
}

pub struct Context {
    id: u64,
    request: Request,
    handlers:Vec<Box<dyn Handle>>
}