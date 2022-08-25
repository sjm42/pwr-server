// main.rs

use askama::Template;
use axum::{extract::Path, http::StatusCode, response::Html, routing::get, Router};
use chrono::*;
use coap::CoAPClient;
use log::*;
use std::{fmt::Display, net::SocketAddr, sync::Arc, time};
use structopt::StructOpt;
use tower_http::trace::TraceLayer;

mod startup;
use startup::*;

#[derive(Template)]
#[template(path = "index.html", escape = "none")]
struct IndexTemplate<'a> {
    cmd_status: &'a str,
    cmd_on: &'a str,
    cmd_off: &'a str,
}

fn main() -> anyhow::Result<()> {
    let mut opts = OptsCommon::from_args();
    opts.finish()?;
    start_pgm(&opts, "pwr_server");

    let index1 = IndexTemplate {
        cmd_status: "/pwr/cmd/status",
        cmd_on: "/pwr/cmd/on",
        cmd_off: "/pwr/cmd/off",
    }
    .render()?;

    let addr = opts.listen.parse::<SocketAddr>().unwrap();
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
    let runtime = tokio::runtime::Runtime::new()?;

    runtime.block_on(async {
        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .await
            .unwrap();
    });

    Ok(())
}

async fn cmd(Path(op): Path<String>, state: Arc<OptsCommon>) -> (StatusCode, String) {
    let mut coap_url = String::with_capacity(state.coap_url.len() + 16);
    coap_url.push_str(state.coap_url.as_str());
    let coap_data = Utc::now().timestamp().to_string();

    match op.as_str() {
        "on" => coap_url.push_str("pwr_on"),
        "off" => coap_url.push_str("pwr_off"),
        _ => coap_url.push_str("pwr_get_t"),
    }

    debug!("CoAP POST: {coap_url} <-- {coap_data}");
    let coap_result =
        CoAPClient::post_with_timeout(&coap_url, coap_data.into_bytes(), time::Duration::new(5, 0));
    if let Err(e) = coap_result {
        return int_err(format!("CoAP error: {e:?}"));
    }
    let response = coap_result.unwrap();
    let msg = String::from_utf8_lossy(&response.message.payload);
    debug!("CoAP reply: \"{msg}\"");

    let indata = msg.split(':').collect::<Vec<&str>>();
    if indata.len() != 2 {
        return int_err(format!("CoAP: invalid response: \"{msg}\""));
    }
    let state_str = if indata[0].eq("1") { "ON" } else { "OFF" };

    let p_res = indata[1].parse::<i64>();
    if let Err(e) = p_res {
        return int_err(format!("CoAP response parse error: {e:?}"));
    }
    let changed = p_res.unwrap();
    let ts_str = Local
        .from_utc_datetime(&NaiveDateTime::from_timestamp(changed, 0))
        .format("%Y-%m-%d %H:%M:%S %Z");

    (
        StatusCode::OK,
        format!("Power {state_str}, last change: {ts_str}"),
    )
}

fn int_err<S: AsRef<str> + Display>(e: S) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
// EOF
