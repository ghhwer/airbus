use airbus::app::AppService;
use airbus::bind_app_service;
use airbus::io::log;
use airbus::io::rpc_server::RpcServer;
use airbus::io::server::parse_listen;
use std::env;
use std::process;
use std::sync::Arc;

fn main() {
    log::info("Airbus!");

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args[1] != "--listen" {
        log::error("usage: airbus --listen [host:]port");
        process::exit(1);
    }

    let spec = if args.len() >= 3 {
        args[2].as_str()
    } else {
        "127.0.0.1:0"
    };

    let (host, port) = match parse_listen(spec) {
        Ok(v) => v,
        Err(e) => {
            log::error(&e);
            process::exit(1);
        }
    };

    let app = Arc::new(AppService::new());
    let mut server = RpcServer::new();
    bind_app_service(&mut server, app);

    if let Err(e) = server.serve_tcp(&host, port) {
        log::error(&e);
        process::exit(1);
    }
}
