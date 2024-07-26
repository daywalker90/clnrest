use std::{net::SocketAddr, str::FromStr};

use axum::{
    http::{HeaderName, HeaderValue},
    middleware,
    routing::{get, post},
    Extension, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use certs::generate_certificates;
use cln_plugin::Builder;
use handlers::{
    call_rpc_method, handle_notification, header_inspection_middleware, list_methods,
    socketio_on_connect,
};
use options::*;
use socketioxide::SocketIo;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tower::ServiceBuilder;
use tower_http::set_header::SetResponseHeaderLayer;

mod certs;
mod handlers;
mod options;
mod shared;

#[derive(Clone, Debug)]
struct PluginState {
    sender: Sender<serde_json::Value>,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    log_panics::init();
    std::env::set_var(
        "CLN_PLUGIN_LOG",
        "cln_plugin=info,cln_rpc=info,clnrest=debug,warn",
    );

    let plugin = match Builder::new(tokio::io::stdin(), tokio::io::stdout())
        .option(OPT_CLNREST_PORT)
        .option(OPT_CLNREST_CERTS)
        .option(OPT_CLNREST_PROTOCOL)
        .option(OPT_CLNREST_HOST)
        .option(OPT_CLNREST_CORS)
        .option(OPT_CLNREST_CSP)
        .option(OPT_CLNREST_SWAGGER)
        .subscribe("*", handle_notification)
        .dynamic()
        .configure()
        .await?
    {
        Some(p) => p,
        None => return Ok(()),
    };

    let clnrest_options = match parse_options(&plugin).await {
        Ok(opts) => opts,
        Err(e) => return plugin.disable(&e.to_string()).await,
    };

    let (tx, rx) = mpsc::channel(100);

    let state = PluginState { sender: tx };

    let plugin = plugin.start(state.clone()).await?;

    let (socket_layer, socket_io) = SocketIo::new_layer();

    socket_io.ns("/", socketio_on_connect);

    tokio::spawn(notification_background_task(socket_io.clone(), rx));

    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .layer(ServiceBuilder::new().layer(middleware::from_fn_with_state(
            plugin.clone(),
            header_inspection_middleware,
        )))
        .layer(socket_layer)
        .nest(
            "/v1",
            Router::new()
                .route("/list-methods", get(list_methods))
                .route("/:rpc_method", post(call_rpc_method))
                .layer(clnrest_options.cors)
                .layer(Extension(plugin.clone()))
                .layer(
                    ServiceBuilder::new().layer(SetResponseHeaderLayer::if_not_present(
                        HeaderName::from_str("Content-Security-Policy")?,
                        HeaderValue::from_str(&clnrest_options.csp)?,
                    )),
                ),
        );

    match clnrest_options.protocol {
        ClnrestProtocol::Https => {
            if !clnrest_options.certs.join("server.pem").exists()
                || !clnrest_options.certs.join("server-key.pem").exists()
                || !clnrest_options.certs.join("client.pem").exists()
                || !clnrest_options.certs.join("client-key.pem").exists()
                || !clnrest_options.certs.join("ca.pem").exists()
                || !clnrest_options.certs.join("ca-key.pem").exists()
            {
                generate_certificates(&clnrest_options.certs, &plugin.option(&OPT_CLNREST_HOST)?)?;
            }

            let config = RustlsConfig::from_pem_file(
                clnrest_options.certs.join("server.pem"),
                clnrest_options.certs.join("server-key.pem"),
            )
            .await?;
            log::info!(
                "REST server running at https://{}",
                clnrest_options.address_str
            );
            axum_server::bind_rustls(clnrest_options.address, config)
                .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                .await?;
        }
        ClnrestProtocol::Http => {
            log::info!(
                "REST server running at http://{}",
                clnrest_options.address_str
            );
            axum_server::bind(clnrest_options.address)
                .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                .await?;
        }
    }

    return Ok(());
}

async fn notification_background_task(io: SocketIo, mut receiver: Receiver<serde_json::Value>) {
    log::debug!("Background task spawned");
    while let Some(notification) = receiver.recv().await {
        match io.emit("message", notification) {
            Ok(_) => (),
            Err(e) => log::warn!("Could not emit notifications from background task: {}", e),
        }
    }
}