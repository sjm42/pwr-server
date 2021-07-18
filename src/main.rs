// main.rs
#![feature(once_cell)]

extern crate coap;
extern crate parking_lot;
extern crate tera;

use actix_web::{get, middleware, web, App, HttpResponse, HttpServer, Result};
use chrono::*;
use coap::CoAPClient;
use log::*;
use parking_lot::*;
use std::{env, lazy::*, time};
use structopt::StructOpt;
use tera::Tera;
use actix_web::http::StatusCode;

static COAP_URL: SyncLazy<RwLock<String>> = SyncLazy::new(|| RwLock::new(String::new()));
static TERA_DIR: SyncLazy<RwLock<String>> = SyncLazy::new(|| RwLock::new(String::new()));
static TERA: SyncLazy<RwLock<Tera>> =
    SyncLazy::new(||
        RwLock::new(match Tera::new(&format!("{}/*.tera", TERA_DIR.read())) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Tera template parsing error: {}", e);
            ::std::process::exit(1);
        }
    }));

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

#[actix_web::main]
async fn main() -> std::io::Result<()> {
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
    info!(
        "pwr-server built from branch: {} commit: {}",
        env!("GIT_BRANCH"),
        env!("GIT_COMMIT")
    );
    info!("Source timestamp: {}", env!("SOURCE_TIMESTAMP"));
    info!("Compiler version: {}", env!("RUSTC_VERSION"));

    {
        let mut u = COAP_URL.write();
        *u = opt.coap_url.clone();
    }
    {
        info!("Template directory: {}", &opt.template_dir);
        let mut d = TERA_DIR.write();
        *d = opt.template_dir.clone();
    }
    info!("Have templates: [{}]", TERA.read().get_template_names().collect::<Vec<_>>().join(", "));

    HttpServer::new(|| App::new()
        .wrap(middleware::Logger::default())
        .service(cmd)
        .route("/", web::get().to(index))
        .route("/pwr/", web::get().to(index)))
        .bind(&opt.listen)?
        .run()
        .await
}

#[allow(unused_mut)]
async fn index() -> Result<HttpResponse>  {
    let mut context = tera::Context::new();
    // we could add variables for the template with context.insert() here
    // but for now, we don't have any (:
    match TERA.read().render("index.html.tera", &context) {
        Err(e) => {
            Ok(HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR)
                   .content_type("text/plain")
                   .body(format!("Template render error: {}", e)))
        },
        Ok(t) => {
            Ok(HttpResponse::build(StatusCode::OK)
                   .content_type("text/html; charset=utf-8")
                   .body(t))
        }
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
        Err(e) => {
            Ok(HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR)
                   .content_type("text/plain")
                   .body(format!("CoAP error: {:?}", e)))
        }
        Ok(resp) => {
            let msg = String::from_utf8_lossy(&resp.message.payload);
            debug!("CoAP reply: \"{}\"", &msg);
            let indata: Vec<&str> = msg.split(':').collect();
            if indata.len() != 2 {
                return Ok(HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR)
                              .content_type("text/plain")
                              .body(format!("CoAP error: invalid response: \"{}\"", &msg)));
            }
            let state = if indata[0].eq("0") { "OFF" } else { "ON" };
            match indata[1].parse::<i64>() {
                Err(e) => {
                    Ok(HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR)
                           .content_type("text/plain")
                           .body(format!("CoAP error: {:?}", e)))
                }
                Ok(changed) => {
                    let ts = NaiveDateTime::from_timestamp(changed, 0);
                    let ts_str = Local.from_utc_datetime(&ts).format("%Y-%m-%d %H:%M:%S %Z");
                    Ok(HttpResponse::build(StatusCode::OK)
                           .content_type("text/plain")
                           .body(format!("Power {}, last change: {}", state, ts_str)))
                }
            }
        }
    }
}
// EOF
