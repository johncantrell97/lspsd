use electrsd::bitcoind::bitcoincore_rpc::RpcApi;
use electrsd::bitcoind::{BitcoinD, Conf, P2P};

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
