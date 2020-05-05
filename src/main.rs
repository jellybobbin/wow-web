use wow_web::{self,Result,router,context};

#[async_std::main]
async fn main(){
    let mut app = wow_web::new();
    app.run().await;
}