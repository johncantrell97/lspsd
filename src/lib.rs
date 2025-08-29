pub mod client;
pub mod utils;
mod versions;

use anyhow::Context;
use ldk_node::bitcoin::secp256k1::PublicKey;
use ldk_node::ChannelDetails;
use log::{debug, error, warn};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration;
use std::{env, fmt, fs, thread};
use tempfile::TempDir;

pub use anyhow;
pub use tempfile;
pub use which;

use crate::client::LspsClient;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LspConfig {
    pub pubkey: PublicKey,
    pub ip_port: String,
    pub token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingAddress {
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaucetRequest {
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenChannelRequest {
    pub pubkey: PublicKey,
    pub ip_port: String,
    pub funding_sats: u64,
    pub push_sats: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenChannelResponse {
    pub user_channel_id: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactChannel {
    pub channel_id: String,
    pub counterparty_node_id: PublicKey,
    pub channel_value_sats: u64,
    pub user_channel_id: u128,
    pub outbound_capacity_msat: u64,
    pub inbound_capacity_msat: u64,
    pub is_channel_ready: bool,
    pub is_usable: bool,
}

impl From<ChannelDetails> for CompactChannel {
    fn from(channel: ChannelDetails) -> Self {
        Self {
            channel_id: channel.channel_id.to_string(),
            counterparty_node_id: channel.counterparty_node_id,
            channel_value_sats: channel.channel_value_sats,
            user_channel_id: channel.user_channel_id.0,
            outbound_capacity_msat: channel.outbound_capacity_msat,
            inbound_capacity_msat: channel.inbound_capacity_msat,
            is_channel_ready: channel.is_channel_ready,
            is_usable: channel.is_usable,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListChannelsResponse {
    pub channels: Vec<CompactChannel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayInvoiceRequest {
    pub invoice: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayInvoiceResponse {
    pub payment_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetInvoiceRequest {
    pub amount_sats: u64,
    pub description: String,
    pub expiry_secs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetInvoiceResponse {
    pub invoice: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetBalanceResponse {
    pub total_onchain_balance_sats: u64,
    pub spendable_onchain_balance_sats: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPaymentResponse {
    pub status: String,
}

#[derive(Debug)]
/// Struct representing the lspsd process with related information
pub struct LspsD {
    /// Process child handle, used to terminate the process when this struct is dropped
    process: Child,
    /// Client
    pub client: client::LspsClient,
    /// Work directory, where the node stores its data
    work_dir: DataDir,
    /// Contains information to connect to this node
    pub params: ConnectParams,
    /// Confing to connect to lsp
    pub lsp_config: LspConfig,
}

#[derive(Debug)]
/// The DataDir struct defining the kind of data directory the node
/// will contain. Data directory can be either persistent, or temporary.
pub enum DataDir {
    /// Persistent Data Directory
    Persistent(PathBuf),
    /// Temporary Data Directory
    Temporary(TempDir),
}

impl DataDir {
    /// Return the data directory path
    fn path(&self) -> PathBuf {
        match self {
            Self::Persistent(path) => path.to_owned(),
            Self::Temporary(tmp_dir) => tmp_dir.path().to_path_buf(),
        }
    }
}

#[derive(Debug, Clone)]
/// Contains all the information to connect to this node
pub struct ConnectParams {
    /// Url of the rpc of the node, useful for other client to connect to the node
    pub api_socket: SocketAddrV4,
    /// p2p connection url, is some if the node started with p2p enabled
    pub lightning_socket: SocketAddrV4,
}

/// All the possible error in this crate
pub enum Error {
    /// Wrapper of io Error
    Io(std::io::Error),
    /// Returned when calling methods requiring a feature to be activated, but it's not
    NoFeature,
    /// Returned when calling methods requiring a env var to exist, but it's not
    NoEnvVar,
    /// Returned when calling methods requiring the lspsd executable but none is found
    /// (no feature, no `lspsd_EXE`, no `lspsd` in `PATH` )
    NoLspsdExecutableFound,
    /// Wrapper of early exit status
    EarlyExit(ExitStatus),
    /// Returned when both tmpdir and staticdir is specified in `Conf` options
    BothDirsSpecified,
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(_) => write!(f, "io::Error"),
            Error::NoFeature => write!(f, "Called a method requiring a feature to be set, but it's not"),
            Error::NoEnvVar => write!(f, "Called a method requiring env var `LSPSD_EXE` to be set, but it's not"),
            Error::NoLspsdExecutableFound =>  write!(f, "`lspsd` executable is required, provide it with one of the following: set env var `LSPSD_EXE` or use a feature like \"22_1\" or have `lspsd` executable in the `PATH`"),
            Error::EarlyExit(e) => write!(f, "The lspsd process terminated early with exit code {}", e),
            Error::BothDirsSpecified => write!(f, "tempdir and staticdir cannot be enabled at same time in configuration options"),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

const LOCAL_IP: Ipv4Addr = Ipv4Addr::new(127, 0, 0, 1);

/// The node configuration parameters, implements a convenient [Default] for most common use.
///
/// `#[non_exhaustive]` allows adding new parameters without breaking downstream users.
/// Users cannot instantiate the struct directly, they need to create it via the `default()` method
/// and mutate fields according to their preference.
///
/// Default values:
/// ```
/// let mut conf = lspsd::Conf::default();
/// conf.args = vec!["--network regtest"];
/// conf.view_stdout = false;
/// conf.network = "regtest";
/// conf.tmpdir = None;
/// conf.staticdir = None;
/// conf.attempts = 3;
/// assert_eq!(conf, lspsd::Conf::default());
/// ```
///
#[non_exhaustive]
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Conf<'a> {
    /// if `true` lspsd log output will not be suppressed
    pub view_stdout: bool,

    /// Must match what specified in args without dashes
    pub network: &'a str,

    /// Optionally specify a temporary or persistent working directory for the node.
    /// The following two parameters can be configured to simulate desired working directory configuration.
    ///
    /// tmpdir is Some() && staticdir is Some() : Error. Cannot be enabled at same time.
    /// tmpdir is Some(temp_path) && staticdir is None : Create temporary directory at `tmpdir` path.
    /// tmpdir is None && staticdir is Some(work_path) : Create persistent directory at `staticdir` path.
    /// tmpdir is None && staticdir is None: Creates a temporary directory in OS default temporary directory (eg /tmp) or `TEMPDIR_ROOT` env variable path.
    ///
    /// It may be useful for example to set to a ramdisk via `TEMPDIR_ROOT` env option so that
    /// lspsd nodes spawn very fast because their datadirs are in RAM. Should not be enabled with persistent
    /// mode, as it cause memory overflows.

    /// Temporary directory path
    pub tmpdir: Option<PathBuf>,

    /// Persistent directory path
    pub staticdir: Option<PathBuf>,

    /// Esplora URL
    pub esplora_url: Option<String>,

    /// RGS Url
    pub rgs_url: Option<String>,

    /// Try to spawn the process `attempt` time
    ///
    /// The OS is giving available ports to use, however, they aren't booked, so it could rarely
    /// happen they are used at the time the process is spawn. When retrying other available ports
    /// are returned reducing the probability of conflicts to negligible.
    pub attempts: u8,
}

impl Default for Conf<'_> {
    fn default() -> Self {
        Conf {
            view_stdout: false,
            network: "regtest",
            tmpdir: None,
            staticdir: None,
            attempts: 3,
            esplora_url: None,
            rgs_url: None,
        }
    }
}

impl LspsD {
    /// Launch the lspsd process from the given `exe` executable with default args.
    ///
    /// Waits for the node to be ready to accept connections before returning
    pub fn new<S: AsRef<OsStr>>(exe: S) -> anyhow::Result<LspsD> {
        LspsD::with_conf(exe, &Conf::default())
    }

    /// Launch the lspsd process from the given `exe` executable with given [Conf] param
    pub fn with_conf<S: AsRef<OsStr>>(exe: S, conf: &Conf) -> anyhow::Result<LspsD> {
        let tmpdir = conf
            .tmpdir
            .clone()
            .or_else(|| env::var("TEMPDIR_ROOT").map(PathBuf::from).ok());
        let work_dir = match (&tmpdir, &conf.staticdir) {
            (Some(_), Some(_)) => return Err(Error::BothDirsSpecified.into()),
            (Some(tmpdir), None) => DataDir::Temporary(TempDir::new_in(tmpdir)?),
            (None, Some(workdir)) => {
                fs::create_dir_all(workdir)?;
                DataDir::Persistent(workdir.to_owned())
            }
            (None, None) => DataDir::Temporary(TempDir::new()?),
        };

        let work_dir_path = work_dir.path();
        debug!("work_dir: {:?}", work_dir_path);

        let mut args = vec![];

        let api_port = get_available_port()?;
        let api_socket = SocketAddrV4::new(LOCAL_IP, api_port);
        let api_url = format!("http://{}", api_socket);

        args.push("--api-port".to_string());
        args.push(format!("{}", api_port));

        let lightning_port = get_available_port()?;
        let lightning_socket = SocketAddrV4::new(LOCAL_IP, lightning_port);

        args.push("--lightning-port".to_string());
        args.push(format!("{}", lightning_port));

        let stdout = if conf.view_stdout {
            Stdio::inherit()
        } else {
            Stdio::null()
        };

        args.push("--network".to_string());
        args.push(format!("{}", conf.network));

        args.push("--data-dir".to_string());
        args.push(format!("{}", work_dir_path.display()));

        if let Some(esplora_url) = &conf.esplora_url {
            args.push("--esplora-url".to_string());
            args.push(format!("{}", esplora_url));
        }

        if let Some(rgs_url) = &conf.rgs_url {
            args.push("--rgs-url".to_string());
            args.push(format!("{}", rgs_url));
        }

        debug!("launching {:?} with args: {:?}", exe.as_ref(), args);

        let mut process = Command::new(exe.as_ref())
            .args(args)
            .stdout(stdout)
            .spawn()
            .with_context(|| format!("Error while executing {:?}", exe.as_ref()))?;

        let mut i = 0;
        // wait lspsd is ready, use default wallet
        let (client, lsp_config) = loop {
            if let Some(status) = process.try_wait()? {
                if conf.attempts > 0 {
                    warn!("early exit with: {:?}. Trying to launch again ({} attempts remaining), maybe some other process used our available port", status, conf.attempts);
                    let mut conf = conf.clone();
                    conf.attempts -= 1;
                    return Self::with_conf(exe, &conf)
                        .with_context(|| format!("Remaining attempts {}", conf.attempts));
                } else {
                    error!("early exit with: {:?}", status);
                    return Err(Error::EarlyExit(status).into());
                }
            }
            thread::sleep(Duration::from_millis(100));
            assert!(process.stderr.is_none());
            let client = LspsClient::new(&api_url);

            if let Ok(lsp_config) = client.get_lsps_config() {
                // TODO: maybe should automatically fund the wallet?
                break (client, lsp_config);
            }

            debug!(
                "bitcoin client for process {} not ready ({})",
                process.id(),
                i
            );

            i += 1;
        };

        Ok(LspsD {
            process,
            client,
            lsp_config,
            work_dir,
            params: ConnectParams {
                api_socket,
                lightning_socket,
            },
        })
    }

    /// Returns the rpc URL including the schema eg. http://127.0.0.1:44842
    pub fn api_url(&self) -> String {
        format!("http://{}", self.params.api_socket)
    }

    /// Return the current workdir path of the running node
    pub fn workdir(&self) -> PathBuf {
        self.work_dir.path()
    }

    /// Stop the node, waiting correct process termination
    pub fn stop(&mut self) -> anyhow::Result<ExitStatus> {
        // TODO: impl stop
        Ok(self.process.wait()?)
    }
}

impl LspsD {
    /// create LspsD struct with the downloaded executable.
    pub fn from_downloaded() -> anyhow::Result<LspsD> {
        LspsD::new(downloaded_exe_path())
    }
    /// create LspsD struct with the downloaded executable and given Conf.
    pub fn from_downloaded_with_conf(conf: &Conf) -> anyhow::Result<LspsD> {
        LspsD::with_conf(downloaded_exe_path(), conf)
    }
}

impl Drop for LspsD {
    fn drop(&mut self) {
        if let DataDir::Persistent(_) = self.work_dir {
            let _ = self.stop();
        }
        let _ = self.process.kill();
    }
}

/// Returns a non-used local port if available.
///
/// Note there is a race condition during the time the method check availability and the caller
pub fn get_available_port() -> anyhow::Result<u16> {
    // using 0 as port let the system assign a port available
    let t = TcpListener::bind(("127.0.0.1", 0))?; // 0 means the OS choose a free port
    Ok(t.local_addr().map(|s| s.port())?)
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// Provide the lspsd executable path if a version feature has been specified
pub fn downloaded_exe_path() -> String {
    let mut path: PathBuf = env!("OUT_DIR").into();
    path.push("lspsd");

    if cfg!(target_os = "windows") {
        path.push("lspsd.exe");
    } else {
        path.push("lspsd");
    }

    format!("{}", path.display())
}

/// Returns the daemon `lspsd` executable with the following precedence:
///
/// 1) If it's specified in the `BITCOIND_EXE` env var
/// 2) If there is no env var but an auto-download feature such as `23_1` is enabled, returns the
/// path of the downloaded executabled
/// 3) If neither of the precedent are available, the `lspsd` executable is searched in the `PATH`
pub fn exe_path() -> anyhow::Result<String> {
    if let Ok(path) = std::env::var("LSPSD_EXE") {
        return Ok(path);
    }
    Ok(downloaded_exe_path())
}

/// Validate the specified arg if there is any unavailable or deprecated one
pub fn validate_args(args: Vec<&str>) -> anyhow::Result<Vec<&str>> {
    Ok(args)
}
