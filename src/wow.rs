use async_std::net::{TcpStream, TcpListener};
use async_std::prelude::*;
use async_std::task;
use http_types::{Response, StatusCode, Request, Method};

pub struct Application{
    pub name: String
}

impl Application{
    pub async fn run(&self) -> http_types::Result<()> {
        let listener = TcpListener::bind(("127.0.0.1", 8080)).await?;
        let addr = format!("http://{}", listener.local_addr()?);
        println!("listening on {}", addr);

        // For each incoming TCP connection, spawn a task and call `accept`.
        let mut incoming = listener.incoming();
        while let Some(stream) = incoming.next().await {
            let stream = stream?;
            let addr = addr.clone();
            task::spawn(async {
                if let Err(err) = accept(addr, stream).await {
                    eprintln!("{}", err);
                }
            });
        }
        Ok(())
    }
}


// Take a TCP stream, and convert it into sequential HTTP request / response pairs.
async fn accept(addr: String, stream: TcpStream) -> http_types::Result<()> {
    println!("starting new connection from {}", stream.peer_addr()?);
    async_h1::accept(&addr, stream.clone(), |mut _req| async move {
        //return Future<Output = http_types::Result<Response>>

        //这个地方的路由怎么加
        //let mut router: Router<Handler> = Router::new();
        _req.set_method(Method::Get);
        println!("{:?}",_req.url());
        let mut res = Response::new(StatusCode::Ok);
        res.insert_header("Content-Type", "text/plain")?;
        res.set_body(_req.url().to_owned().to_string());
        Ok(res)
    })
        .await?;
    Ok(())
}