use airbus::app::AppService;
use airbus::bind_app_service;
use airbus::io::http_server;
use airbus::io::log;
use airbus::io::rpc_server::RpcServer;
use airbus::io::server::parse_listen;
use std::env;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::thread;

struct Cli {
    listen: String,
    http: Option<String>,
    resources: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> Result<Cli, String> {
    if args.len() < 2 {
        return Err(usage());
    }

    let mut listen: Option<String> = None;
    let mut http: Option<String> = None;
    let mut resources: Option<PathBuf> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--listen" => {
                i += 1;
                let spec = args
                    .get(i)
                    .ok_or_else(|| "missing value for --listen".to_string())?;
                listen = Some(spec.clone());
            }
            "--http" => {
                i += 1;
                let spec = args
                    .get(i)
                    .ok_or_else(|| "missing value for --http".to_string())?;
                http = Some(spec.clone());
            }
            "--resources" => {
                i += 1;
                let path = args
                    .get(i)
                    .ok_or_else(|| "missing value for --resources".to_string())?;
                resources = Some(PathBuf::from(path));
            }
            other => return Err(format!("unknown argument: {other}\n{}", usage())),
        }
        i += 1;
    }

    let listen = listen.ok_or_else(usage)?;
    if http.is_some() && resources.is_none() {
        return Err("--http requires --resources DIR".into());
    }
    if resources.is_some() && http.is_none() {
        return Err("--resources requires --http [host:]port".into());
    }

    Ok(Cli {
        listen,
        http,
        resources,
    })
}

fn usage() -> String {
    "usage: airbus --listen [host:]port [--http [host:]port --resources DIR]".into()
}

fn main() {
    log::info("Airbus!");

    let args: Vec<String> = env::args().collect();
    let cli = match parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            log::error(&e);
            process::exit(1);
        }
    };

    let (host, port) = match parse_listen(&cli.listen) {
        Ok(v) => v,
        Err(e) => {
            log::error(&e);
            process::exit(1);
        }
    };

    let app = Arc::new(AppService::new());
    let mut server = RpcServer::new();
    bind_app_service(&mut server, app);
    let server = Arc::new(server);

    if let (Some(http_spec), Some(resources)) = (cli.http, cli.resources) {
        let (http_host, http_port) = match parse_listen(&http_spec) {
            Ok(v) => v,
            Err(e) => {
                log::error(&e);
                process::exit(1);
            }
        };
        let rpc = Arc::clone(&server);
        thread::spawn(move || {
            if let Err(e) = http_server::serve_http(&http_host, http_port, &resources, |bytes| {
                rpc.on_bytes(bytes)
            }) {
                log::error(&e);
                process::exit(1);
            }
        });
    }

    if let Err(e) = server.serve_tcp(&host, port) {
        log::error(&e);
        process::exit(1);
    }
}
