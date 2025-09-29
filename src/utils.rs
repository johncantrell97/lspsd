use electrsd::bitcoind::bitcoincore_rpc::RpcApi;
use electrsd::bitcoind::{BitcoinD, Conf, P2P};
use cdk::nuts::CurrencyUnit;
use std::sync::Arc;
use ldk_node::lightning::ln::msgs::SocketAddress;
use cdk_ldk_node::{BitcoinRpcConfig, GossipSource};
use std::net::SocketAddr;
use ldk_node::bitcoin::secp256k1::PublicKey;
use cdk::types::FeeReserve;
use cdk::mint::{MintBuilder, MintMeltLimits};
use std::str::FromStr;
use ldk_node::bitcoin::Network;
use rand::RngCore;


use std::time::Duration;



pub fn get_bitcoind() -> BitcoinD {
    let mut conf = Conf::default();
    conf.p2p = P2P::Yes;
    BitcoinD::from_downloaded_with_conf(&conf).unwrap()
}

pub fn get_funded_bitcoind() -> BitcoinD {
    let bitcoind = get_bitcoind();
    let address = bitcoind
        .client
        .get_new_address(None, None)
        .unwrap()
        .assume_checked();
    bitcoind.client.generate_to_address(101, &address).unwrap();
    bitcoind
}

pub fn get_esplorad(bitcoind: &BitcoinD) -> electrsd::ElectrsD {
    let electrs_exe = electrsd::exe_path().expect("electrs version feature must be enabled");
    let mut conf = electrsd::Conf::default();
    conf.http_enabled = true;
    electrsd::ElectrsD::with_conf(electrs_exe, bitcoind, &conf).unwrap()
}

pub fn generate_blocks(bitcoind: &BitcoinD, num: u64) {
	let address = bitcoind.client.get_new_address(None, None).unwrap().assume_checked();
	let _block_hashes = bitcoind.client.generate_to_address(num, &address).unwrap();
}

pub fn start_cashu_mint(
    bitcoind: Arc<BitcoinD>, 
    storage_dir: String, 
    rt: Arc<tokio::runtime::Runtime>,
    lsp_node_id: PublicKey,
    lsp_listen: SocketAddress
) {
    let cookie = bitcoind.params.get_cookie_values().unwrap().unwrap();
    let bitcoind_port = bitcoind.params.rpc_socket.port();
    let cdk_port = {
        let t = std::net::TcpListener::bind(("0.0.0.0", 0)).unwrap();
        t.local_addr().unwrap().port()
    };
    let cdk_addr = SocketAddr::from_str(format!("0.0.0.0:{cdk_port}").as_str()).unwrap();
    let cdk = cdk_ldk_node::CdkLdkNode::new(
        Network::Regtest,
        cdk_ldk_node::ChainSource::BitcoinRpc(BitcoinRpcConfig {
            host: "127.0.0.1".to_string(),
            port: bitcoind_port,
            user: cookie.user.clone(),
            password: cookie.password.clone(),
        }),
        GossipSource::P2P,
        storage_dir,
        FeeReserve { min_fee_reserve: Default::default(), percent_fee_reserve: 0.0 },
        vec![cdk_addr.into()],
        Some(rt.clone()),
    )
    .unwrap();
    let cdk = Arc::new(cdk);

    let mint_addr = {
        let t = std::net::TcpListener::bind(("0.0.0.0", 0)).unwrap();
        let port = t.local_addr().unwrap().port();
        SocketAddr::from_str(format!("0.0.0.0:{port}").as_str()).unwrap()
    };

    println!("Cashu Port: {}", mint_addr.port());

    let bitcoind_clone = Arc::clone(&bitcoind);
    let lsp_listen_clone = lsp_listen.clone();
    let _mint = rt.block_on(async move {
        // build mint
        let mem_db = Arc::new(cdk_sqlite::mint::memory::empty().await.unwrap());
        let mut mint_seed: [u8; 64] = [0; 64];
        rand::thread_rng().fill_bytes(&mut mint_seed);
        let mut builder = MintBuilder::new(mem_db.clone());

        builder
            .add_payment_processor(
                CurrencyUnit::Sat,
                cdk::nuts::PaymentMethod::Bolt11,
                MintMeltLimits::new(0, u64::MAX),
                cdk.clone(),
            )
            .await
            .unwrap();

        builder
            .add_payment_processor(
                CurrencyUnit::Sat,
                cdk::nuts::PaymentMethod::Bolt12,
                MintMeltLimits::new(0, u64::MAX),
                cdk.clone(),
            )
            .await
            .unwrap();

        let mint = Arc::new(builder.build_with_seed(mem_db, &mint_seed).await.unwrap());

        mint.start().await.unwrap();

        let listener = tokio::net::TcpListener::bind(mint_addr).await.unwrap();

        let v1_service = cdk_axum::create_mint_router(Arc::clone(&mint), true).await.unwrap();

        let axum_result = axum::serve(listener, v1_service);

        tokio::spawn(async move {
            if let Err(e) = axum_result.await {
                eprintln!("Error running mint axum server: {e}");
            }
        });

        // open channel from cashu ldk node to lsp
        let addr = cdk.node().onchain_payment().new_address().unwrap();
        let addr = electrsd::bitcoind::bitcoincore_rpc::bitcoin::Address::from_str(
            &addr.to_string(),
        )
        .unwrap()
        .assume_checked();
        bitcoind_clone
            .client
            .send_to_address(&addr, electrsd::bitcoind::bitcoincore_rpc::bitcoin::amount::Amount::from_btc(3.0).unwrap(), None, None, None, None, None, None)
            .unwrap();
        generate_blocks(&bitcoind_clone, 6);
        tokio::time::sleep(Duration::from_secs(5)).await; // wait for sync
        cdk.node()
            .open_channel(lsp_node_id, lsp_listen_clone, 16_000_000, Some(8_000_000_000), None)
            .unwrap();
        // wait for tx to broadcast
        tokio::time::sleep(Duration::from_secs(1)).await;
        generate_blocks(&bitcoind_clone, 10);

        // wait for sync/channel ready
        for _ in 0..10 {
            if cdk.node().list_channels().first().is_some_and(|c| c.is_usable) {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        mint
    });
}
