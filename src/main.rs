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
    let mut coap_url = data.o.coap_url.clone();
    let coap_data = Utc::now().timestamp().to_string();

    match op.as_str() {
        "on" => coap_url.push_str("pwr_on"),
        "off" => coap_url.push_str("pwr_off"),
        _ => coap_url.push_str("pwr_get_t"),
    }

    debug!("CoAP POST: {} <-- {}", &coap_url, &coap_data);
    let coap_result =
        CoAPClient::post_with_timeout(&coap_url, coap_data.into_bytes(), time::Duration::new(5, 0));
    if let Err(e) = coap_result {
        return int_err(format!("CoAP error: {:?}", e));
    }
    let response = coap_result.unwrap();
    let msg = String::from_utf8_lossy(&response.message.payload);
    debug!("CoAP reply: \"{}\"", &msg);

    let indata = msg.split(':').collect::<Vec<&str>>();
    if indata.len() != 2 {
        return int_err(format!("CoAP: invalid response: \"{}\"", &msg));
    }
    let state = if indata[0].eq("1") { "ON" } else { "OFF" };

    let p_res = indata[1].parse::<i64>();
    if let Err(e) = p_res {
        return int_err(format!("CoAP response parse error: {:?}", e));
    }
    let changed = p_res.unwrap();
    let ts_str = Local
        .from_utc_datetime(&NaiveDateTime::from_timestamp(changed, 0))
        .format("%Y-%m-%d %H:%M:%S %Z");
    Ok(HttpResponse::build(StatusCode::OK)
        .content_type(TEXT_PLAIN)
        .body(format!("Power {}, last change: {}", state, ts_str)))
}

fn int_err(e: String) -> Result<HttpResponse> {
    Ok(HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR)
        .content_type(TEXT_PLAIN)
        .body(e))
}
// EOF
