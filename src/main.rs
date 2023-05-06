// main.rs

use askama::Template;
use axum::{extract::Path, http::StatusCode, response::Html, routing::get, Router};
use chrono::*;
use coap::CoAPClient;
use log::*;
use std::{fmt::Display, net::SocketAddr, sync::Arc, thread, time};
use structopt::StructOpt;
use tower_http::trace::TraceLayer;

mod config;
use config::*;

#[derive(Template)]
#[template(path = "index.html", escape = "none")]
struct IndexTemplate<'a> {
    cmd_status: &'a str,
    cmd_on: &'a str,
    cmd_off: &'a str,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut opts = OptsCommon::from_args();
    opts.finish()?;
    start_pgm(&opts, "pwr_server");

    let index1 = IndexTemplate {
        cmd_status: "/pwr/cmd/status",
        cmd_on: "/pwr/cmd/on",
        cmd_off: "/pwr/cmd/off",
    }
    .render()?;

    let addr = opts.listen.parse::<SocketAddr>()?;
    let shared_state = Arc::new(opts);

    let app = Router::new()
        .route(
            "/",
            get({
                let index = index1.clone();
                move || async { Html(index) }
            }),
        )
        .route("/pwr", get(move || async { Html(index1) }))
        .route(
            "/pwr/cmd/:op",
            get({
                let state = Arc::clone(&shared_state);
                move |path| cmd(path, Arc::clone(&state))
            }),
        )
        .layer(TraceLayer::new_for_http());

    Ok(axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?)
}

const TS_NONE: &str = "(none)";
async fn cmd(Path(op): Path<String>, state: Arc<OptsCommon>) -> (StatusCode, String) {
    thread::spawn(move || {
        let mut coap_url = String::with_capacity(state.coap_url.len() + 16);
        coap_url.push_str(state.coap_url.as_str());
        let coap_data = Utc::now().timestamp().to_string();

        match op.as_str() {
            "on" => coap_url.push_str("pwr_on"),
            "off" => coap_url.push_str("pwr_off"),
            _ => coap_url.push_str("pwr_get_t"),
        }

        debug!("CoAP POST: {coap_url} <-- {coap_data}");
        let response = match CoAPClient::post_with_timeout(
            &coap_url,
            coap_data.into_bytes(),
            time::Duration::new(5, 0),
        ) {
            Err(e) => {
                return int_err(format!("CoAP error: {e:#?}"));
            }
            Ok(r) => r,
        };

        let msg = String::from_utf8_lossy(&response.message.payload);
        debug!("CoAP reply: \"{msg}\"");

        let indata = msg.split(':').collect::<Vec<&str>>();
        if indata.len() != 2 {
            return int_err(format!("CoAP: invalid response: \"{msg}\""));
        }
        let state_str = if indata[0].eq("1") { "ON" } else { "OFF" };

        let changed = match indata[1].parse::<i64>() {
            Err(e) => {
                return int_err(format!("CoAP response parse error: {e:#?}"));
            }
            Ok(p) => p,
        };

        let ts_str = match NaiveDateTime::from_timestamp_opt(changed, 0) {
            Some(naive_ts) if changed != 0 => {
                let dt = Local.from_utc_datetime(&naive_ts);
                dt.format("%Y-%m-%d %H:%M:%S %Z").to_string()
            }
            _ => TS_NONE.to_string(),
        };

        let status = format!("Power {state_str}, last change: {ts_str}");
        info!("Status: {status}");
        (StatusCode::OK, status)
    })
    .join()
    .unwrap_or_else(|e| int_err(format!("Thread join error: {e:#?}")))
}

fn int_err<S: AsRef<str> + Display>(e: S) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

// EOF
