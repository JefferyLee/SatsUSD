//! Node-set environment resolution: one switch selects which node
//! set (regtest devnet or public signet) the harnesses talk to.
//!
//! `SATUSD_NET=devnet` (default) or `SATUSD_NET=signet`. Ports and
//! paths mirror `devnet/env.sh` / `signet/env.sh` — change those,
//! change this.

use std::path::PathBuf;
use std::process::Command;

use crate::TapChannel;

#[derive(Clone, Debug)]
pub struct NodeEnv {
    /// Bitcoin chain name, used for the bitcoin-cli flag and lnd
    /// macaroon path ("regtest" | "signet").
    pub chain: &'static str,
    pub root: PathBuf,
    pub btc_datadir: PathBuf,
    pub btc_rpc_port: u16,
    pub btc_wallet: &'static str,
    pub tapd_addr: &'static str,
    pub tapd_dir: PathBuf,
    pub tapd_tls: PathBuf,
    pub tapd_macaroon: PathBuf,
    pub lnd_addr: &'static str,
    pub lnd_tls: PathBuf,
    pub lnd_macaroon: PathBuf,
}

impl NodeEnv {
    /// Resolve from `SATUSD_NET` with paths relative to the given
    /// workspace root.
    pub fn from_env(root: PathBuf) -> Self {
        let net = std::env::var("SATUSD_NET").unwrap_or_else(|_| "devnet".into());
        match net.as_str() {
            "devnet" | "regtest" => NodeEnv {
                chain: "regtest",
                btc_datadir: root.join("devnet/data/bitcoind"),
                btc_rpc_port: 18443,
                btc_wallet: "regtest",
                tapd_addr: "127.0.0.1:10029",
                tapd_dir: root.join("devnet/data/tapd"),
                tapd_tls: root.join("devnet/data/tapd/tls.cert"),
                tapd_macaroon: root.join("devnet/data/tapd/data/regtest/admin.macaroon"),
                lnd_addr: "127.0.0.1:10009",
                lnd_tls: root.join("devnet/data/lnd/tls.cert"),
                lnd_macaroon: root
                    .join("devnet/data/lnd/data/chain/bitcoin/regtest/admin.macaroon"),
                root,
            },
            "signet" => NodeEnv {
                chain: "signet",
                btc_datadir: root.join("signet/data/bitcoind"),
                btc_rpc_port: 38332,
                btc_wallet: "signet",
                tapd_addr: "127.0.0.1:10030",
                tapd_dir: root.join("signet/data/tapd"),
                tapd_tls: root.join("signet/data/tapd/tls.cert"),
                tapd_macaroon: root.join("signet/data/tapd/data/signet/admin.macaroon"),
                lnd_addr: "127.0.0.1:10010",
                lnd_tls: root.join("signet/data/lnd/tls.cert"),
                lnd_macaroon: root.join("signet/data/lnd/data/chain/bitcoin/signet/admin.macaroon"),
                root,
            },
            other => panic!("unknown SATUSD_NET '{other}' (expected devnet|signet)"),
        }
    }

    /// Non-panicking bitcoin-cli (polling loops, races expected).
    pub fn try_bcli(&self, args: &[&str]) -> Result<serde_json::Value, String> {
        let mut cmd = Command::new("bitcoin-cli");
        cmd.args([
            format!("-{}", self.chain),
            format!("-datadir={}", self.btc_datadir.display()),
            "-rpcuser=satusd".to_string(),
            "-rpcpassword=satusd".to_string(),
            format!("-rpcport={}", self.btc_rpc_port),
            format!("-rpcwallet={}", self.btc_wallet),
        ]);
        cmd.args(args);
        let out = cmd.output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).into_owned());
        }
        let s = String::from_utf8_lossy(&out.stdout);
        Ok(serde_json::from_str(s.trim())
            .unwrap_or_else(|_| serde_json::Value::String(s.trim().to_string())))
    }

    /// bitcoin-cli against this node set's wallet. Panics on failure
    /// (harness semantics — these run inside tests and ops tools).
    pub fn bcli(&self, args: &[&str]) -> serde_json::Value {
        let mut cmd = Command::new("bitcoin-cli");
        cmd.args([
            format!("-{}", self.chain),
            format!("-datadir={}", self.btc_datadir.display()),
            "-rpcuser=satusd".to_string(),
            "-rpcpassword=satusd".to_string(),
            format!("-rpcport={}", self.btc_rpc_port),
            format!("-rpcwallet={}", self.btc_wallet),
        ]);
        cmd.args(args);
        let out = cmd.output().expect("bitcoin-cli runs");
        assert!(
            out.status.success(),
            "bitcoin-cli {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        let s = String::from_utf8(out.stdout).expect("utf8");
        serde_json::from_str(s.trim())
            .unwrap_or_else(|_| serde_json::Value::String(s.trim().to_string()))
    }

    /// tapcli against this node set (binaries are shared in devnet/bin).
    pub fn tapcli(&self, args: &[&str]) -> String {
        let out = Command::new(self.root.join("devnet/bin/tapcli"))
            .args([
                format!("--network={}", self.chain),
                format!("--rpcserver={}", self.tapd_addr),
                format!("--tapddir={}", self.tapd_dir.display()),
            ])
            .args(args)
            .output()
            .expect("tapcli runs");
        assert!(
            out.status.success(),
            "tapcli {:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("utf8")
    }

    pub async fn tapd_channel(&self) -> Result<TapChannel, Box<dyn std::error::Error>> {
        let tls = std::fs::read(&self.tapd_tls)?;
        let mac = std::fs::read(&self.tapd_macaroon)?;
        crate::connect(self.tapd_addr, &tls, &hex::encode(&mac), "localhost").await
    }

    pub async fn lnd_channel(&self) -> Result<TapChannel, Box<dyn std::error::Error>> {
        let tls = std::fs::read(&self.lnd_tls)?;
        let mac = std::fs::read(&self.lnd_macaroon)?;
        crate::connect(self.lnd_addr, &tls, &hex::encode(&mac), "localhost").await
    }
}
