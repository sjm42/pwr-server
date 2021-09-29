// main.rs

use actix_web::{
    get, http::StatusCode, middleware, web, App, HttpResponse, HttpServer, Responder, Result,
};
use askama::Template;
use chrono::*;
use coap::CoAPClient;
use log::*;
use std::time;
use structopt::StructOpt;

mod startup;
use startup::*;

const TEXT_PLAIN: &str = "text/plain; charset=utf-8";
const TEXT_HTML: &str = "text/html; charset=utf-8";

#[derive(Template)]
#[template(path = "index.html", escape = "none")]
struct IndexTemplate<'a> {
    cmd_status: &'a str,
    cmd_on: &'a str,
    cmd_off: &'a str,
}

#[derive(Clone)]
struct RuntimeConfig {
    o: OptsCommon,
    index_html: String,
}

fn main() -> anyhow::Result<()> {
    let mut opts = OptsCommon::from_args();
    opts.finish()?;
    start_pgm(&opts, "pwr-server");

    // initialize runtime config
    let data = web::Data::new(RuntimeConfig {
        o: opts.clone(),
        index_html: IndexTemplate {
            cmd_status: "/pwr/cmd/status",
            cmd_on: "/pwr/cmd/on",
            cmd_off: "/pwr/cmd/off",
        }
        .render()?,
    });
    actix_web::rt::System::new("pwr-server").block_on(async move {
        HttpServer::new(move || {
            App::new()
                .app_data(data.clone())
                .wrap(middleware::Logger::default())
                .service(cmd)
                .route("/", web::get().to(index))
                .route("/pwr/", web::get().to(index))
        })
        .bind(&opts.listen)?
        .run()
        .await
    })?;
    Ok(())
}

async fn index(data: web::Data<RuntimeConfig>) -> impl Responder {
    HttpResponse::build(StatusCode::OK)
        .content_type(TEXT_HTML)
        .body(data.index_html.clone())
}

#[get("/pwr/cmd/{op}")]
async fn cmd(path: web::Path<(String,)>, data: web::Data<RuntimeConfig>) -> impl Responder {
    let (op,) = path.into_inner();
    let url_pre = &data.o.coap_url;
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
    match CoAPClient::post_with_timeout(&url, coap_data.into_bytes(), time::Duration::new(5, 0)) {
        Err(e) => int_err(format!("CoAP error: {:?}", e)),
        Ok(resp) => {
            let msg = String::from_utf8_lossy(&resp.message.payload);
            debug!("CoAP reply: \"{}\"", &msg);
            let indata: Vec<&str> = msg.split(':').collect();
            if indata.len() != 2 {
                return int_err(format!("CoAP: invalid response: \"{}\"", &msg));
            }
            let state = if indata[0].eq("1") { "ON" } else { "OFF" };

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
