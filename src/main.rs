use std::process::Command;
use std::str::FromStr;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::post;
use axum::Json;
use axum::{routing::get, Router};
use electrsd::bitcoind::bitcoincore_rpc::RpcApi;
use hex_conservative::DisplayHex;
use hex_conservative::FromHex;
use ldk_node::lightning::ln::msgs::SocketAddress;
use ldk_node::lightning::ln::PaymentHash;
use ldk_node::lightning_invoice::Bolt11Invoice;
use ldk_node::Event;
use ldk_node::Node;
use ldk_node::{bitcoin::Network, Builder, Config};
use lspsd::client::LspsClient;
use serde_json::{json, Value};
use tokio::runtime::Runtime;

use argh::FromArgs;
use lspsd::{
    utils, FaucetRequest, FundingAddress, GetBalanceResponse, GetInvoiceRequest,
    GetInvoiceResponse, GetPaymentResponse, ListChannelsResponse, LspConfig, OpenChannelRequest,
    OpenChannelResponse, PayInvoiceRequest, PayInvoiceResponse,
};

#[derive(FromArgs)]
/// Arguments to start the lsp daemon
struct LspArgs {
    /// data directory used to store node info
    #[argh(option)]
    data_dir: String,
    /// what bitcoin network to operate on
    #[argh(option)]
    network: Option<Network>,
    /// what p2p port to listen on
    #[argh(option)]
    lightning_port: u16,
    /// what port to use for the http api
    #[argh(option)]
    api_port: u16,
    /// what esplora server to use
    #[argh(option)]
    esplora_url: Option<String>,
    /// what rgs server to use
    #[argh(option)]
    rgs_url: Option<String>,
    /// optional lspsd faucet to get funds from
    #[argh(option)]
    lspsd_faucet_url: Option<String>,
}
#[derive(Clone)]
struct AppState {
    node: Arc<Node>,
    bitcoin: Option<Arc<electrsd::bitcoind::BitcoinD>>,
    esplora: Option<Arc<electrsd::ElectrsD>>,
}

fn main() {
    let args: LspArgs = argh::from_env();

    let mut config = Config::default();
    config.storage_dir_path = args.data_dir.clone();
    config.network = args.network.unwrap_or(Network::Regtest);
    config.listening_addresses = Some(vec![SocketAddress::TcpIpV4 {
        addr: [0, 0, 0, 0],
        port: args.lightning_port,
    }]);

    let (esplora_url, bitcoin, esplora) = match args.esplora_url {
        Some(esplora_url) => (esplora_url, None, None),
        None => {
            if config.network != Network::Regtest {
                panic!("esplora url is required");
            }
            let bitcoind = utils::get_funded_bitcoind();
            let esplora = utils::get_esplorad(&bitcoind);
            let esplora_url = format!("http://{}", esplora.esplora_url.clone().unwrap());

            std::fs::remove_dir_all(args.data_dir.clone()).unwrap();
            std::fs::remove_dir_all(format!("{}.child", &args.data_dir)).unwrap();

            println!(
                "no esplora_url provided, started a server at: {}",
                esplora_url
            );

            (
                esplora_url,
                Some(Arc::new(bitcoind)),
                Some(Arc::new(esplora)),
            )
        }
    };

    let mut builder = Builder::from_config(config);
    builder.set_esplora_server(esplora_url.clone());
    builder.set_liquidity_provider_lsps2();

    if let Some(rgs_url) = args.rgs_url {
        builder.set_gossip_source_rgs(rgs_url);
    } else {
        builder.set_gossip_source_p2p();
    }

    let node = builder.build_with_fs_store().unwrap();

    node.start().unwrap();

    // if no esplora url was given, then we started our own so lets fund ourselves
    if let (Some(bitcoin), Some(esplora)) = (&bitcoin, &esplora) {
        println!(
            "{}@127.0.0.1:{}",
            node.node_id().to_string(),
            args.lightning_port
        );
        let funding_address = node.new_onchain_address().unwrap();
        let funding_address = electrsd::bitcoind::bitcoincore_rpc::bitcoin::Address::from_str(
            &funding_address.to_string(),
        )
        .unwrap()
        .assume_checked();
        let amount =
            electrsd::bitcoind::bitcoincore_rpc::bitcoin::amount::Amount::from_btc(40.0).unwrap();
        let _funding_txid = bitcoin
            .client
            .send_to_address(&funding_address, amount, None, None, None, None, None, None)
            .unwrap();

        let miner_address = bitcoin
            .client
            .get_new_address(None, None)
            .unwrap()
            .assume_checked();
        bitcoin
            .client
            .generate_to_address(1, &miner_address)
            .unwrap();
        let height = bitcoin.client.get_blockchain_info().unwrap().blocks as usize;
        esplora.wait_height(height);
        node.sync_wallets().unwrap();

        let child_data_dir = format!("{}.child", &args.data_dir);
        let child_lightning_port = format!("{}", args.lightning_port + 1);
        let child_api_port = format!("{}", args.api_port + 1);
        let lspsd_faucet_url = format!("http://localhost:{}", args.api_port);
        let child_args = vec![
            "--data-dir",
            &child_data_dir,
            "--lightning-port",
            &child_lightning_port,
            "--api-port",
            &child_api_port,
            "--esplora-url",
            &esplora_url,
            "--lspsd-faucet-url",
            &lspsd_faucet_url,
        ];

        let _child = Command::new(std::env::current_exe().unwrap())
            .args(&child_args)
            .spawn()
            .expect("failed to spawn child process");
    }

    // if a faucet url was given, we can fund our node from there and then open a channel to them
    if let Some(lspsd_faucet_url) = args.lspsd_faucet_url {
        println!("node started with a faucet provided: {}", lspsd_faucet_url);

        let ip_port = format!("127.0.0.1:{}", args.lightning_port);
        let faucet_client = LspsClient::new(&lspsd_faucet_url);
        faucet_client
            .open_channel(
                node.node_id(),
                SocketAddress::from_str(&ip_port).unwrap(),
                16_000_000,
                8_000_000,
            )
            .unwrap();

        node.sync_wallets().unwrap();

        println!(
            "node now has a channel with the faucet: {:?}",
            node.list_balances()
        );
    }

    let app_state = AppState {
        node: Arc::new(node),
        bitcoin,
        esplora,
    };
    let app = Router::new()
        .route("/config", get(config_handler))
        .route("/funding-address", get(funding_address))
        .route("/faucet", post(faucet))
        .route("/channels", post(open_channel))
        .route("/channels", get(list_channels))
        .route("/pay-invoice", post(pay_invoice))
        .route("/get-invoice", post(get_invoice))
        .route("/sync", post(sync))
        .route("/balance", get(get_balance))
        .route("/get-payment/:payment_hash", get(get_payment))
        .with_state(app_state);

    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", args.api_port))
            .await
            .unwrap();

        println!("started http server listening on port: {}", args.api_port);

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

async fn faucet(State(state): State<AppState>, Json(req): Json<FaucetRequest>) -> Json<String> {
    let address = ldk_node::bitcoin::Address::from_str(&req.address)
        .unwrap()
        .assume_checked();
    let txid = state
        .node
        .send_to_onchain_address(&address, 100_000_000)
        .unwrap();

    if let Some(esplora) = &state.esplora {
        if let Some(bitcoin) = &state.bitcoin {
            let miner_address = bitcoin
                .client
                .get_new_address(None, None)
                .unwrap()
                .assume_checked();
            bitcoin
                .client
                .generate_to_address(1, &miner_address)
                .unwrap();
            let info = bitcoin.client.get_blockchain_info().unwrap();
            esplora.wait_height(info.blocks as usize);
        }
    }
    Json(txid.to_string())
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

    if let Some(esplora) = &state.esplora {
        if let Some(bitcoin) = &state.bitcoin {
            loop {
                let event = state.node.wait_next_event();

                if let Event::ChannelPending { .. } = event {
                    let miner_address = bitcoin
                        .client
                        .get_new_address(None, None)
                        .unwrap()
                        .assume_checked();
                    bitcoin
                        .client
                        .generate_to_address(6, &miner_address)
                        .unwrap();
                    let info = bitcoin.client.get_blockchain_info().unwrap();
                    esplora.wait_height(info.blocks as usize);
                    state.node.sync_wallets().unwrap();
                }

                if let Event::ChannelReady { .. } = event {
                    state.node.event_handled();
                    break;
                }

                state.node.event_handled();
            }
        }
    }

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
        payment_hash: res.0.to_lower_hex_string(),
    })
}

async fn get_invoice(
    State(state): State<AppState>,
    Json(req): Json<GetInvoiceRequest>,
) -> Json<GetInvoiceResponse> {
    let invoice = state
        .node
        .receive_payment(req.amount_sats * 1000, &req.description, req.expiry_secs)
        .unwrap();

    Json(GetInvoiceResponse {
        invoice: invoice.to_string(),
    })
}

async fn sync(State(state): State<AppState>) -> Json<Value> {
    state.node.sync_wallets().unwrap();
    Json(json!({"synced": true}))
}

async fn get_balance(State(state): State<AppState>) -> Json<GetBalanceResponse> {
    let balances = state.node.list_balances();
    Json(GetBalanceResponse {
        total_onchain_balance_sats: balances.total_onchain_balance_sats,
        spendable_onchain_balance_sats: balances.spendable_onchain_balance_sats,
    })
}

async fn get_payment(
    State(state): State<AppState>,
    Path(payment_hash): Path<String>,
) -> Json<GetPaymentResponse> {
    let payment_hash_bytes = <[u8; 32]>::from_hex(&payment_hash).unwrap();
    let payment_hash = PaymentHash(payment_hash_bytes);
    let payment = state.node.payment(&payment_hash).unwrap();

    Json(GetPaymentResponse {
        status: match payment.status {
            ldk_node::PaymentStatus::Pending => "pending".to_string(),
            ldk_node::PaymentStatus::Succeeded => "succeeded".to_string(),
            ldk_node::PaymentStatus::Failed => "failed".to_string(),
        },
        preimage: payment
            .preimage
            .map(|preimage| preimage.0.to_lower_hex_string()),
    })
}
