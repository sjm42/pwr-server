// main.rs
#![feature(once_cell)]

extern crate coap;
// extern crate log;
extern crate parking_lot;
#[macro_use]
extern crate rocket;
// extern crate simplelog;
extern crate tera;

use chrono::*;
use coap::CoAPClient;
// use log::*;
use parking_lot::*;
// use rocket::{Build, Rocket};
use rocket_dyn_templates::Template;
// use simplelog::*;
use std::{lazy::*, time};
use structopt::StructOpt;
// use tera::Context;

static COAP_URL: SyncLazy<RwLock<String>> = SyncLazy::new(|| RwLock::new(String::new()));

#[derive(Debug, StructOpt)]
/// Note: internal InfluxDB client is used unless --influx-binary option is set.
pub struct GlobalServerOptions {
    #[structopt(short, long, default_value = "127.0.0.1")]
    pub address: String,
    #[structopt(short, long, default_value = "8080")]
    pub port: u32,
    #[structopt(short, long, default_value = "coap://127.0.0.1/")]
    pub coap_url: String,
    #[structopt(long, default_value = "templates")]
    pub template_dir: String,
    #[structopt(short, long)]
    pub debug: bool,
    #[structopt(short, long)]
    pub trace: bool,
}

#[rocket::main]
async fn main() {
    let opt = GlobalServerOptions::from_args();
    /*
    let loglevel = if opt.trace {
        LevelFilter::Trace
    } else if opt.debug {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };
    SimpleLogger::init(
        loglevel,
        ConfigBuilder::new()
            .set_time_format_str("%Y-%m-%d %H:%M:%S %Z")
            .build(),
    ).unwrap();
    */

    {
        let mut u = COAP_URL.write();
        *u = opt.coap_url.clone();
    }
    let config = rocket::Config::figment()
        .merge(("address", &opt.address))
        .merge(("port", opt.port))
        .merge(("template_dir", &opt.template_dir))
        .merge(("log_level", if opt.debug { "debug" } else { "normal" }));

    let r = rocket::custom(config)
        .mount("/", routes![index, cmd])
        .attach(Template::fairing());
    info!(
        "pwr-server built from branch: {} commit: {}",
        env!("GIT_BRANCH"),
        env!("GIT_COMMIT")
    );
    info!("Source timestamp: {}", env!("SOURCE_TIMESTAMP"));
    info!("Compiler version: {}", env!("RUSTC_VERSION"));
    let ret = r.launch().await;
    match ret {
        Ok(()) => {
            info!("Program ended.");
        }
        Err(e) => {
            error!("Program aborted: {:?}", e);
        }
    }
}

#[allow(unused_mut)]
#[get("/")]
fn index() -> Template {
    let mut context = tera::Context::new();
    // we could add variables for the template with context.insert() here
    // but for now, we don't have any (:
    Template::render("index", &context.into_json())
}

#[get("/cmd/<op>")]
fn cmd(op: &str) -> String {
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
            format!("CoAP error: {:?}", e)
        }
        Ok(resp) => {
            let msg = String::from_utf8_lossy(&resp.message.payload);
            debug!("CoAP reply: \"{}\"", &msg);
            let indata: Vec<&str> = msg.split(':').collect();
            if indata.len() != 2 {
                return format!("CoAP error: invalid response: \"{}\"", &msg);
            }
            let state = if indata[0].eq("0") { "OFF" } else { "ON" };
            match indata[1].parse::<i64>() {
                Err(e) => {
                    format!("CoAP error: {:?}", e)
                }
                Ok(changed) => {
                    let ts = NaiveDateTime::from_timestamp(changed, 0);
                    let ts_str = Local.from_utc_datetime(&ts).format("%Y-%m-%d %H:%M:%S %Z");
                    format!("Power {}, last change: {}", state, ts_str)
                }
            }
        }
    }
}
// EOF
