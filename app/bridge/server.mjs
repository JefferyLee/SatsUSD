// SatUSD signing bridge — the "node signs" half of "phone verifies, node signs".
//
// The PWA verifies a quote entirely on-device, then (only on the user's tap)
// POSTs here to actually sign + broadcast the redemption. We wrap the existing,
// proven `satusd redeem` CLI rather than reimplement the J3 flow: it builds the
// anchor PSBT via tapd, signs the BTC leg via lnd, has the LP co-sign, and
// broadcasts — re-running its OWN verification too (defense in depth: the node
// refuses a bad quote independently of the phone).
//
// Binds to 127.0.0.1 only; reached via the Vite dev proxy (/bridge). Inputs are
// passed to the CLI as an argv array (execFile, no shell) and validated, so
// there is no shell-injection surface.
import { createServer } from "node:http";
import { execFile } from "node:child_process";
import { readFileSync, writeFileSync, unlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const SATUSD_BIN = resolve(REPO_ROOT, "target/release/satusd");
const LP = process.env.LP_URL ?? "http://207.148.98.132:9595";
const ORACLE = process.env.ORACLE_URL ?? "http://207.148.98.132:9590";
const PORT = Number(process.env.BRIDGE_PORT ?? 9596);

// Demo guard rails: small amounts only, signet addresses only.
const MAX_USD = 25;
const ADDR_RE = /^(tb1|bcrt1)[0-9a-z]{20,90}$/;

// Vault (devnet) guard rails: collateral 0.0001–10 BTC, mint play-money cap.
const MIN_COLLATERAL_SATS = 10_000;
const MAX_COLLATERAL_SATS = 1_000_000_000;
const MAX_MINT_USD = 100_000;

// Vault registry — the positions the wallet shows. Persisted to disk so
// they survive a bridge restart (a real wallet would scan the chain for
// live Qs; this is the demo's stand-in).
const VAULTS_FILE = resolve(dirname(fileURLToPath(import.meta.url)), "vaults.json");
const vaults = [];
try {
  for (const v of JSON.parse(readFileSync(VAULTS_FILE, "utf8"))) vaults.push(v);
} catch { /* no registry yet */ }
function saveVaults() {
  try { writeFileSync(VAULTS_FILE, JSON.stringify(vaults, null, 2)); }
  catch (e) { console.error("[bridge] vaults save failed:", e.message); }
}


function json(res, code, obj) {
  const body = JSON.stringify(obj);
  res.writeHead(code, { "content-type": "application/json", "content-length": Buffer.byteLength(body) });
  res.end(body);
}

function readBody(req) {
  return new Promise((ok, err) => {
    let b = "";
    req.on("data", (c) => {
      b += c;
      if (b.length > 32768) req.destroy();
    });
    req.on("end", () => ok(b));
    req.on("error", err);
  });
}

function runRedeem({ amount_usd, payout_address, quote }) {
  return new Promise((ok) => {
    const args = ["redeem", LP, amount_usd.toFixed(2), "--payout", payout_address, "--oracle", ORACLE];
    // Reuse the quote the phone already verified (write to a temp file and
    // pass --quote-file) so redeem doesn't POST a second /v0/quote that
    // would deadlock on the LP's already-locked UTXO.
    let quoteFile;
    if (quote && typeof quote === "object") {
      quoteFile = resolve(tmpdir(), `satusd-quote-${process.pid}-${reqSeq++}.json`);
      writeFileSync(quoteFile, JSON.stringify(quote));
      args.push("--quote-file", quoteFile);
    }
    execFile(
      SATUSD_BIN,
      args,
      { cwd: REPO_ROOT, env: { ...process.env, SATUSD_NET: "signet", SATUSD_ROOT: REPO_ROOT }, timeout: 120_000 },
      (e, stdout, stderr) => {
        if (quoteFile) { try { unlinkSync(quoteFile); } catch { /* best effort */ } }
        const out = (stdout || "") + (stderr || "");
        const m = out.match(/BROADCAST:\s*([0-9a-f]{64})/i);
        if (m) return ok({ ok: true, txid: m[1], log: out.trim().slice(-2000) });
        ok({ ok: false, error: (stderr || e?.message || "redeem failed").trim().slice(-800), log: out.trim().slice(-2000) });
      },
    );
  });
}
let reqSeq = 0;

// vault-open runs on signet (same chain as burn/transfer/balance) so the
// minted SatUSD reissues into the canonical group the wallet reads — the
// mint is broadcast and confirms on signet's schedule (returns pending).
function runVaultOpen({ collateral_sats, mint_usd }) {
  return new Promise((ok) => {
    const args = ["vault-open", String(collateral_sats), mint_usd.toFixed(2), "--oracle", ORACLE];
    execFile(
      SATUSD_BIN,
      args,
      { cwd: REPO_ROOT, env: { ...process.env, SATUSD_NET: "signet", SATUSD_ROOT: REPO_ROOT }, timeout: 120_000 },
      (e, stdout, stderr) => {
        const out = (stdout || "") + (stderr || "");
        const m = out.match(/VAULT:\s*(\{.*\})\s*$/m);
        if (m) {
          try { return ok({ ...JSON.parse(m[1]), log: out.trim().slice(-2000) }); } catch { /* fallthrough */ }
        }
        ok({ ok: false, error: (stderr || e?.message || "vault-open failed").trim().slice(-800), log: out.trim().slice(-2000) });
      },
    );
  });
}

// The node's SatUSD balance on signet (the wallet's headline figure).
function runBalance() {
  return new Promise((ok) => {
    execFile(
      SATUSD_BIN,
      ["balance"],
      { cwd: REPO_ROOT, env: { ...process.env, SATUSD_NET: "signet", SATUSD_ROOT: REPO_ROOT }, timeout: 30_000 },
      (e, stdout, stderr) => {
        const out = (stdout || "") + (stderr || "");
        const m = out.match(/total:\s*(\d+)/);
        const btc = out.match(/BTC_SATS:\s*(\d+)\s+(\d+)/);
        const pend = out.match(/PENDING_MICRO:\s*(\d+)/);
        if (m)
          return ok({
            ok: true,
            micro: Number(m[1]),
            pending_micro: pend ? Number(pend[1]) : 0,
            btc_sats: btc ? Number(btc[1]) : 0,
            btc_pending_sats: btc ? Number(btc[2]) : 0,
          });
        ok({ ok: false, error: (stderr || e?.message || "balance failed").trim().slice(-400) });
      },
    );
  });
}

// A fresh signet deposit address — the wallet's Receive view.
function runAddress() {
  return new Promise((ok) => {
    execFile(
      SATUSD_BIN,
      ["address"],
      { cwd: REPO_ROOT, env: { ...process.env, SATUSD_NET: "signet", SATUSD_ROOT: REPO_ROOT }, timeout: 30_000 },
      (e, stdout, stderr) => {
        const out = (stdout || "") + (stderr || "");
        const m = out.match(/ADDRESS:\s*(\S+)/);
        if (m) return ok({ ok: true, address: m[1] });
        ok({ ok: false, error: (stderr || e?.message || "address failed").trim().slice(-400) });
      },
    );
  });
}

const server = createServer(async (req, res) => {
  if (req.method === "GET" && req.url === "/health") return json(res, 200, { ok: true, lp: LP });
  if (req.method === "GET" && req.url === "/vaults") return json(res, 200, { ok: true, vaults });
  if (req.method === "GET" && req.url === "/balance") return json(res, 200, await runBalance());
  if (req.method === "GET" && req.url === "/address") return json(res, 200, await runAddress());

  if (req.method === "POST" && req.url === "/redeem") {
    let body;
    try { body = JSON.parse(await readBody(req)); } catch { return json(res, 400, { error: "bad JSON" }); }

    const amount_usd = Number(body.amount_usd);
    const payout_address = String(body.payout_address ?? "");
    if (!(amount_usd > 0) || amount_usd > MAX_USD)
      return json(res, 400, { error: `amount must be 0 < usd <= ${MAX_USD}` });
    if (!ADDR_RE.test(payout_address))
      return json(res, 400, { error: "payout_address is not a signet bech32 address" });

    console.log(`[bridge] redeem $${amount_usd.toFixed(2)} -> ${payout_address}${body.quote ? " (reusing verified quote)" : ""}`);
    const result = await runRedeem({ amount_usd, payout_address, quote: body.quote });
    console.log(`[bridge] ${result.ok ? "BROADCAST " + result.txid : "FAIL " + result.error}`);
    return json(res, result.ok ? 200 : 502, result);
  }

  if (req.method === "POST" && req.url === "/vault/open") {
    let body;
    try { body = JSON.parse(await readBody(req)); } catch { return json(res, 400, { error: "bad JSON" }); }

    const collateral_sats = Math.round(Number(body.collateral_sats));
    const mint_usd = Number(body.mint_usd);
    if (!Number.isInteger(collateral_sats) || collateral_sats < MIN_COLLATERAL_SATS || collateral_sats > MAX_COLLATERAL_SATS)
      return json(res, 400, { error: `collateral_sats must be ${MIN_COLLATERAL_SATS}..${MAX_COLLATERAL_SATS}` });
    if (!(mint_usd > 0) || mint_usd > MAX_MINT_USD)
      return json(res, 400, { error: `mint_usd must be 0 < usd <= ${MAX_MINT_USD}` });

    console.log(`[bridge] vault-open ${collateral_sats} sats / $${mint_usd}`);
    const result = await runVaultOpen({ collateral_sats, mint_usd });
    if (result.ok) {
      vaults.unshift({
        vault_id: result.vault_id,
        q: result.q,
        collateral_sats: result.collateral_sats,
        mint_micro_usd: result.mint_micro_usd,
        funding_txid: result.funding_txid,
        funding_vout: result.funding_vout,
        net: result.net,
        opened_at: Date.now(),
      });
      saveVaults();
    }
    console.log(`[bridge] ${result.ok ? "VAULT " + String(result.vault_id).slice(0, 16) : "FAIL " + result.error}`);
    return json(res, result.ok ? 200 : 502, result);
  }

  return json(res, 404, { error: "not found" });
});

server.listen(PORT, "127.0.0.1", () => {
  console.log(`[bridge] satusd signing bridge on http://127.0.0.1:${PORT}  (LP ${LP}, net signet)`);
  console.log(`[bridge] binary ${SATUSD_BIN}`);
});
