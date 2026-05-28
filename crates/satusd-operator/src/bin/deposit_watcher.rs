//! Deposit confirmation watcher (M6 pilot, playbook §1.C). Scans bitcoind for
//! UTXOs at the reserve address and reports each one's depth. UTXOs with depth
//! ≥ `min_confirmations` are flagged READY together with the `MintCommitWitness`
//! inputs the issuer needs; the rest are PENDING with a `n/k` confirmations
//! count. Wrap in `watch`/cron, or pass a watch interval as the third arg to
//! loop in-process.
//!
//!   deposit_watcher <reserve_address> [min_confirmations=6] [watch_seconds=0]
//!
//! Env: SATUSD_BITCOIND_URL (default http://127.0.0.1:18443/wallet/regtest),
//!      SATUSD_BITCOIND_USER (default satusd), SATUSD_BITCOIND_PASS (default satusd).
//!
//! Status (PRD §5.D11/M2 modeling boundary):
//!   `mint_commit` accepts `deposit_txid`, `deposit_sats`, and
//!   `deposit_confirmations` as **witness facts** — the state node trusts them.
//!   The production tightening is **in-state SPV verification of the deposit
//!   on-chain** (same machinery `satusd-state::spv` already uses for redemption
//!   payouts, DL-22). This watcher is the operational glue **today**; the
//!   protocol upgrade is post-MVP and tracked in `SECURITY.md` §3 + the playbook.

use std::process::ExitCode;
use std::str::FromStr;
use std::{thread, time::Duration};

use bitcoin::Address;
use bitcoincore_rpc::{json::ScanTxOutRequest, Auth, Client, RpcApi};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "usage: {} <reserve_address> [min_confirmations=6] [watch_seconds=0]",
            args[0]
        );
        return ExitCode::from(2);
    }
    let reserve_addr_str = &args[1];
    let min_confirmations: u64 = args
        .get(2)
        .map(|s| s.parse().expect("min_confirmations u64"))
        .unwrap_or(6);
    let watch_seconds: u64 = args
        .get(3)
        .map(|s| s.parse().expect("watch_seconds u64"))
        .unwrap_or(0);

    let url = std::env::var("SATUSD_BITCOIND_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:18443/wallet/regtest".to_string());
    let user = std::env::var("SATUSD_BITCOIND_USER").unwrap_or_else(|_| "satusd".to_string());
    let pass = std::env::var("SATUSD_BITCOIND_PASS").unwrap_or_else(|_| "satusd".to_string());
    let btc = match Client::new(&url, Auth::UserPass(user, pass)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[deposit-watcher] bitcoind connect failed: {e}");
            return ExitCode::from(1);
        }
    };

    // Validate the address parses (network-checked by the user; the descriptor
    // bitcoind builds is just the text, so a wrong network address scans empty).
    if Address::from_str(reserve_addr_str).is_err() {
        eprintln!("[deposit-watcher] bad reserve address: {reserve_addr_str}");
        return ExitCode::from(2);
    }

    let watch_disp = if watch_seconds == 0 {
        "off".to_string()
    } else {
        format!("{watch_seconds}s")
    };
    println!(
        "[deposit-watcher] reserve={reserve_addr_str}  min_confirmations={min_confirmations}  watch={watch_disp}"
    );

    loop {
        if let Err(e) = scan_and_report(&btc, reserve_addr_str, min_confirmations) {
            eprintln!("[deposit-watcher] scan failed: {e}");
            if watch_seconds == 0 {
                return ExitCode::from(1);
            }
        }
        if watch_seconds == 0 {
            return ExitCode::SUCCESS;
        }
        thread::sleep(Duration::from_secs(watch_seconds));
    }
}

fn scan_and_report(btc: &Client, reserve_addr: &str, min_confirmations: u64) -> Result<(), String> {
    let req = ScanTxOutRequest::Single(format!("addr({reserve_addr})"));
    let scan = btc
        .scan_tx_out_set_blocking(&[req])
        .map_err(|e| format!("scantxoutset: {e}"))?;
    let tip = btc.get_block_count().map_err(|e| e.to_string())?;

    if scan.unspents.is_empty() {
        println!("[deposit-watcher] tip={tip} no deposits at reserve");
        return Ok(());
    }
    for u in &scan.unspents {
        let confs = tip.saturating_sub(u.height).saturating_add(1);
        let ready = confs >= min_confirmations;
        let tag = if ready {
            "READY".to_string()
        } else {
            format!("PENDING {confs}/{min_confirmations}")
        };
        println!(
            "[deposit-watcher] tx {}:{}  {} sat  {} conf  [{tag}]",
            u.txid,
            u.vout,
            u.amount.to_sat(),
            confs,
        );
        if ready {
            // Print the MintCommitWitness inputs the issuer needs to fill in.
            println!(
                "    ↪ MintCommitWitness inputs:\n      deposit_txid          = {}\n      deposit_sats          = {}\n      deposit_confirmations = {}",
                u.txid,
                u.amount.to_sat(),
                confs,
            );
        }
    }
    Ok(())
}
