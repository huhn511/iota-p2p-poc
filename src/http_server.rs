
use std::net::SocketAddr;
use std::net::{IpAddr, Ipv4Addr};

use actix_web::{middleware, web, App, HttpRequest, HttpServer, HttpResponse, delete, get, head, options, patch, post, put};


#[actix_web::main]
pub async fn start_server() -> std::io::Result<()> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8000);

    println!("Runnning HTTP Server on http://{}", addr);
    // std::env::set_var("RUST_LOG", "actix_web=info");
    // env_logger::init();

    HttpServer::new(|| {
        App::new()
            .wrap(middleware::Logger::default())
            .service(index)
            .service(handle_ping)
    })
        .bind(&addr)?
        .run()
        .await
}

#[get("/")]
async fn index() -> HttpResponse {
    HttpResponse::Ok().body("Welcome to iota-p2p-poc!")
}

#[get("/ping")]
async fn handle_ping() -> HttpResponse {
    HttpResponse::Ok().body("pong")
}