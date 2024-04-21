use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use ldk_node::bitcoin::secp256k1::PublicKey;
use ldk_node::lightning_persister::fs_store::FilesystemStore;
use ldk_node::Node;
use ldk_node::{Config, Builder, bitcoin::Network};
use ldk_node::lightning::ln::msgs::SocketAddress;
use axum::{
    routing::get,
    Router,
};

use argh::FromArgs;
use serde::Serialize;
use serde_json::{json, Value};

#[derive(FromArgs)]
/// Arguments to start the lsp daemon
struct LspArgs {
    /// what bitcoin network to operate on
    #[argh(option)]
    network: Network,
    /// what p2p port to listen on
    #[argh(option)]
    lightning_port: u16,
    /// what port to use for the http api
    #[argh(option)]
    api_port: u16,
    /// what esplora server to use
    #[argh(option)]
    esplora_url: String,
    /// what rgs server to use
    #[argh(option)]
    rgs_url: Option<String>,
}
#[derive(Clone)]
struct AppState {
    node: Arc<Node<FilesystemStore>>,
}

#[tokio::main]
async fn main() {
    let args: LspArgs = argh::from_env();

    let mut config = Config::default();
    config.network = args.network;
    config.listening_addresses = Some(vec![SocketAddress::TcpIpV4 { addr: [0,0,0,0], port: args.lightning_port }]);
    
    let mut builder = Builder::from_config(config);
    builder.set_esplora_server(args.esplora_url);
    builder.set_liquidity_provider_lsps2();

    if let Some(rgs_url) = args.rgs_url {
        builder.set_gossip_source_rgs(rgs_url);
    } else {
        builder.set_gossip_source_p2p();
    }

    let node = builder.build_with_fs_store().unwrap();

    node.start().unwrap();

    let app_state = AppState { node: Arc::new(node) };
    let app = Router::new()
        .route("/config", get(config_handler))
        .route("/funding-address", get(funding_address))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", args.api_port))
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();

}


#[derive(Debug, Serialize, Clone)]
pub struct LspConfig {
    pub pubkey: PublicKey,
    pub ip_port: String,
    pub token: Option<String>
}

async fn config_handler(
    State(state): State<AppState>,
) -> Json<LspConfig> {
    let lsp_config = LspConfig {
        pubkey: state.node.node_id(),
        ip_port: state.node.listening_addresses().unwrap()[0].to_string(),
        token: None,
    };

    Json(lsp_config)
}

async fn funding_address(
    State(state): State<AppState>,
) -> Json<Value> {
    Json(json!({"address" : state.node.new_onchain_address().unwrap().to_string()}))
}