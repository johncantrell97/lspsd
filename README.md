## LSPS based LSP Daemon and library for testing

You can run it as a daemon if you want but really it's intended to be used as a library in your integration tests.

```rust
// get bitcoind
let mut bitcoind_conf = bitcoind::Conf::default();
bitcoind_conf.p2p = P2P::Yes;
let bitcoind = BitcoinD::from_downloaded_with_conf(&conf).unwrap()

// get electrs w/ "esplora" http server
let electrs_exe = electrsd::exe_path().unwrap();
let mut electrs_conf = electrsd::Conf::default();
electrs_conf.http_enabled = true;
let esplora = electrsd::ElectrsD::with_conf(electrs_exe, bitcoind, &electrs_conf).unwrap()

// get lspsd
let esplora_url = esplora.esplora_url.clone().unwrap();
let lspsd_exe = lspsd::exe_path().unwrap();
let mut lspsd_conf = lspsd::Conf::default();
lspsd_conf.esplora_url = Some(format!("http://{}", esplora_url.to_string()));
let lsp = lspsd::LspsD::with_conf(lspsd_exe, &conf).unwrap();

// use lsp.client to open channels, sync the node, etc.

```