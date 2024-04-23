use std::str::FromStr;
use std::sync::Arc;

use axum::extract::State;
use axum::routing::post;
use axum::Json;
use axum::{routing::get, Router};
use ldk_node::bitcoin::secp256k1::PublicKey;
use ldk_node::lightning::ln::msgs::SocketAddress;
use ldk_node::lightning::ln::ChannelId;
use ldk_node::lightning_invoice::Bolt11Invoice;
use ldk_node::lightning_persister::fs_store::FilesystemStore;
use ldk_node::{bitcoin::Network, Builder, Config};
use ldk_node::{ChannelDetails, Node};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::runtime::Runtime;

use argh::FromArgs;
use lspsd::{
    FundingAddress, ListChannelsResponse, LspConfig, OpenChannelRequest, OpenChannelResponse,
    PayInvoiceRequest, PayInvoiceResponse,
};

#[derive(FromArgs)]
/// Arguments to start the lsp daemon
struct LspArgs {
    /// data directory used to store node info
    #[argh(option)]
    data_dir: String,
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

fn main() {
    let args: LspArgs = argh::from_env();

    let mut config = Config::default();
    config.storage_dir_path = args.data_dir;
    config.network = args.network;
    config.listening_addresses = Some(vec![SocketAddress::TcpIpV4 {
        addr: [0, 0, 0, 0],
        port: args.lightning_port,
    }]);

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

    let app_state = AppState {
        node: Arc::new(node),
    };
    let app = Router::new()
        .route("/config", get(config_handler))
        .route("/funding-address", get(funding_address))
        .route("/channels", post(open_channel))
        .route("/channels", get(list_channels))
        .route("/invoices", post(pay_invoice))
        .route("/sync", post(sync))
        .with_state(app_state);

    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", args.api_port))
            .await
            .unwrap();

        axum::serve(listener, app).await.unwrap();
    });
}

async fn config_handler(State(state): State<AppState>) -> Json<LspConfig> {
    let lsp_config = LspConfig {
        pubkey: state.node.node_id(),
        ip_port: state.node.listening_addresses().unwrap()[0].to_string(),
        token: None,
    };

    Json(lsp_config)
}

async fn funding_address(State(state): State<AppState>) -> Json<FundingAddress> {
    Json(FundingAddress {
        address: state.node.new_onchain_address().unwrap().to_string(),
    })
}

async fn open_channel(
    State(state): State<AppState>,
    Json(req): Json<OpenChannelRequest>,
) -> Json<OpenChannelResponse> {
    let socket_addr = SocketAddress::from_str(&req.ip_port).unwrap();
    let res = state
        .node
        .connect_open_channel(
            req.pubkey,
            socket_addr,
            req.funding_sats,
            Some(req.push_sats * 1000),
            None,
            true,
        )
        .unwrap();

    Json(OpenChannelResponse {
        user_channel_id: res.0,
    })
}

async fn list_channels(State(state): State<AppState>) -> Json<ListChannelsResponse> {
    let channels = state
        .node
        .list_channels()
        .into_iter()
        .map(|channel| channel.into())
        .collect::<Vec<_>>();

    Json(ListChannelsResponse { channels })
}

async fn pay_invoice(
    State(state): State<AppState>,
    Json(req): Json<PayInvoiceRequest>,
) -> Json<PayInvoiceResponse> {
    let invoice = Bolt11Invoice::from_str(&req.invoice).unwrap();
    let res = state.node.send_payment(&invoice).unwrap();
    Json(PayInvoiceResponse {
        payment_hash: res.to_string(),
    })
}

async fn sync(
    State(state): State<AppState>
) -> Json<Value> {
    state.node.sync_wallets().unwrap();
    Json(json!({"synced": true}))
}