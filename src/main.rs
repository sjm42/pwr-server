// main.rs
#![feature(once_cell)]

use actix_web::{get, http::StatusCode, middleware, web, App, HttpResponse, HttpServer, Result};
use chrono::*;
use coap::CoAPClient;
use log::*;
use parking_lot::*;
use std::{env, fmt::Debug, io, lazy::*, time};
use structopt::StructOpt;
use tera::Tera;

const INDEX_TEMPLATE: &str = "index.html.tera";
const TEXT_PLAIN: &str = "text/plain; charset=utf-8";
const TEXT_HTML: &str = "text/html; charset=utf-8";

#[derive(Debug, StructOpt)]
pub struct GlobalServerOptions {
    #[structopt(short, long)]
    pub debug: bool,
    #[structopt(short, long)]
    pub trace: bool,
    #[structopt(short, long, default_value = "127.0.0.1:8080")]
    pub listen: String,
    #[structopt(short, long, default_value = "coap://127.0.0.1/")]
    pub coap_url: String,
    #[structopt(long, default_value = "templates")]
    pub template_dir: String,
}

static COAP_URL: SyncLazy<RwLock<String>> = SyncLazy::new(|| RwLock::new(String::new()));
static TERA_DIR: SyncLazy<RwLock<String>> = SyncLazy::new(|| RwLock::new(String::new()));
static TERA: SyncLazy<RwLock<Tera>> = SyncLazy::new(|| {
    RwLock::new(match Tera::new(&format!("{}/*.tera", TERA_DIR.read())) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Tera template parsing error: {}", e);
            ::std::process::exit(1);
        }
    })
});

#[actix_web::main]
async fn main() -> io::Result<()> {
    let opt = GlobalServerOptions::from_args();
    let loglevel = if opt.trace {
        LevelFilter::Trace
    } else if opt.debug {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };

    // env::set_var("RUST_LOG", "actix_web=debug,actix_server=info");
    env_logger::Builder::new()
        .filter_level(loglevel)
        .format_timestamp_secs()
        .init();
    info!("Starting up pwr-server");
    debug!("Git branch: {}", env!("GIT_BRANCH"));
    debug!("Git commit: {}", env!("GIT_COMMIT"));
    debug!("Source timestamp: {}", env!("SOURCE_TIMESTAMP"));
    debug!("Compiler version: {}", env!("RUSTC_VERSION"));
    {
        let mut u = COAP_URL.write();
        *u = opt.coap_url.clone();
    }
    {
        info!("Template directory: {}", &opt.template_dir);
        let mut d = TERA_DIR.write();
        *d = opt.template_dir.clone();
    }
    info!(
        "Found templates: [{}]",
        TERA.read()
            .get_template_names()
            .collect::<Vec<_>>()
            .join(", ")
    );
    if !TERA
        .read()
        .get_template_names()
        .any(|t| t.eq(INDEX_TEMPLATE))
    {
        error!("Required template {} not found. Exit.", INDEX_TEMPLATE);
        return Err(io::Error::new(io::ErrorKind::NotFound, "File not found"));
    }

    HttpServer::new(|| {
        App::new()
            .wrap(middleware::Logger::default())
            .service(cmd)
            .route("/", web::get().to(index))
            .route("/pwr/", web::get().to(index))
    })
    .bind(&opt.listen)?
    .run()
    .await
}

#[allow(unused_mut)]
async fn index() -> Result<HttpResponse> {
    let mut context = tera::Context::new();
    // we could add variables for the template with context.insert() here
    // but for now, we don't have any (:
    match TERA.read().render(INDEX_TEMPLATE, &context) {
        Err(e) => int_err(format!("Template render error: {}", e)),
        Ok(t) => Ok(HttpResponse::build(StatusCode::OK)
            .content_type(TEXT_HTML)
            .body(t)),
    }
}

#[get("/pwr/cmd/{op}")]
async fn cmd(path: web::Path<(String,)>) -> Result<HttpResponse> {
    let (op,) = path.into_inner();
    let url_pre = COAP_URL.read();
    let url;
    let coap_data = Utc::now().timestamp().to_string();

    if op.eq("on") {
        url = format!("{}{}", &url_pre, "pwr_on");
    } else if op.eq("off") {
        url = format!("{}{}", &url_pre, "pwr_off");
    } else {
        url = format!("{}{}", &url_pre, "pwr_get_t");
    }
    debug!("CoAP POST: {} <-- {}", &url, &coap_data);
    let res =
        CoAPClient::post_with_timeout(&url, coap_data.into_bytes(), time::Duration::from_secs(5));

    match res {
        Err(e) => int_err(format!("CoAP error: {:?}", e)),
        Ok(resp) => {
            let msg = String::from_utf8_lossy(&resp.message.payload);
            debug!("CoAP reply: \"{}\"", &msg);
            let indata: Vec<&str> = msg.split(':').collect();
            if indata.len() != 2 {
                return int_err(format!("CoAP: invalid response: \"{}\"", &msg));
            }
            let state = if indata[0].eq("0") { "OFF" } else { "ON" };

            match indata[1].parse::<i64>() {
                Err(e) => int_err(format!("CoAP response error: {:?}", e)),
                Ok(changed) => {
                    let ts = NaiveDateTime::from_timestamp(changed, 0);
                    let ts_str = Local.from_utc_datetime(&ts).format("%Y-%m-%d %H:%M:%S %Z");
                    Ok(HttpResponse::build(StatusCode::OK)
                        .content_type(TEXT_PLAIN)
                        .body(format!("Power {}, last change: {}", state, ts_str)))
                }
            }
        }
    }
}

fn int_err(e: String) -> Result<HttpResponse> {
    Ok(HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR)
        .content_type(TEXT_PLAIN)
        .body(e))
}
// EOF
