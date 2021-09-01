// main.rs
#![feature(once_cell)]

use actix_web::{get, http::StatusCode, middleware, web, App, HttpResponse, HttpServer, Result};
use askama::Template;
use chrono::*;
use coap::CoAPClient;
use log::*;
use parking_lot::*;
use std::{env, error::Error, fmt::Debug, lazy::*, time};
use structopt::StructOpt;

const TEXT_PLAIN: &str = "text/plain; charset=utf-8";
const TEXT_HTML: &str = "text/html; charset=utf-8";

#[derive(Template)]
#[template(path = "index.html", escape = "none")]
struct IndexTemplate<'a> {
    cmd_status: &'a str,
    cmd_on: &'a str,
    cmd_off: &'a str,
}

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
}

static COAP_URL: SyncLazy<RwLock<String>> = SyncLazy::new(|| RwLock::new(String::new()));
static TEMPLATE: SyncLazy<RwLock<IndexTemplate>> = SyncLazy::new(|| {
    RwLock::new(IndexTemplate {
        cmd_status: "/pwr/cmd/status",
        cmd_on: "/pwr/cmd/on",
        cmd_off: "/pwr/cmd/off",
    })
});
static INDEX: SyncLazy<RwLock<String>> = SyncLazy::new(|| RwLock::new(String::new()));

fn main() -> Result<(), Box<dyn Error>> {
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
        // initialize globals
        let html = TEMPLATE.read().render()?;
        let mut i = INDEX.write();
        *i = html;
        let mut u = COAP_URL.write();
        *u = opt.coap_url.clone();
    }

    actix_web::rt::System::new("pwr-server").block_on(async move {
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
    })?;
    Ok(())
}

async fn index() -> Result<HttpResponse> {
    Ok(HttpResponse::build(StatusCode::OK)
        .content_type(TEXT_HTML)
        .body(&*INDEX.read()))
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
