use clap::{App, Arg};
use tokio::main;
use frills::server::FrillsServer;

#[tokio::main]
async fn main() {
    let port = get_port();

    println!("Running on 0.0.0.0:{}", port);

    let server = FrillsServer::new(port);

    server.run().await;
}

fn get_port() -> u16 {
    let matches = App::new("Frills Server")
        .author("d0nut")
        .version("0.1")
        .about("Runs a frills server on your public network interface.")
        .arg(Arg::with_name("port")
            .short("p")
            .long("port")
            .value_name("PORT")
            .help("The port on which to run the server.")
            .takes_value(true))
        .get_matches();

    match matches.value_of("port").unwrap_or("12345").parse::<u16>() {
        Ok(port) => port,
        Err(_) => 12345
    }
}