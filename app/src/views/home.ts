// Wallet home — the hub. A persistent oracle trust header (the signed
// price every check verifies against), the node's SatUSD balance, your
// open vault positions (each with a CR the phone recomputes live from
// the signed price), and shortcuts into Mint / Send / Redeem.
import { verifiedOraclePrice } from "@satusd/sdk";
import { crBps, crashPriceCeiling, DEFAULT_TERMS } from "@satusd/sdk/vault";

const ORACLE = location.origin + "/oracle";
const BRIDGE = location.origin + "/bridge";

const fmtUsd = (n: number) =>
  "$" + n.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
const fmtBtc = (sats: number) => (sats / 1e8).toFixed(8);

interface Vault {
  vault_id: string;
  q: string;
  collateral_sats: number;
  mint_micro_usd: number;
  funding_txid: string;
  funding_vout: number;
  net: string;
  opened_at: number;
}

let price = 0;
let positionsData: Vault[] | null = null;
let positionsEl: HTMLElement | null = null;

export function renderHome(el: HTMLElement) {
  price = 0;
  positionsData = null;
  positionsEl = null;
  el.innerHTML = `
    <div class="card" id="oracle-card">
      <h2>Oracle price <span class="spin"></span></h2>
      <p class="hint">The signed price every check verifies against.</p>
    </div>
    <div class="card balances" id="balance-card">
      <div class="bal-grid">
        <div class="bal-col">
          <div class="bal-label">SatUSD</div>
          <div class="bal-amt" id="bal">···</div>
          <div class="bal-pending" id="sat-pending"></div>
        </div>
        <div class="bal-col">
          <div class="bal-label">Bitcoin</div>
          <div class="bal-amt" id="btc">···</div>
          <div class="bal-pending" id="btc-pending"></div>
        </div>
      </div>
      <p class="hint bal-foot" id="bal-sub">held by your node on signet</p>
    </div>
    <div class="card">
      <h2>Receive</h2>
      <p class="hint">Fund your wallet — send signet BTC to this address.</p>
      <div class="addr mono" id="deposit" title="tap to copy">loading…</div>
    </div>
    <div class="card">
      <h2>Vault positions</h2>
      <div id="positions"><p class="hint">Loading…</p></div>
    </div>
    <div class="actions">
      <a class="act" href="#birth"><span class="a-ico">+</span><span>Mint</span><small>vault</small></a>
      <a class="act" href="#transfer"><span class="a-ico">✓</span><span>Verify</span><small>transfer</small></a>
      <a class="act" href="#burn"><span class="a-ico">⟳</span><span>Redeem</span><small>burn</small></a>
    </div>
  `;
  loadOracle(el.querySelector("#oracle-card")!);
  loadBalance(el.querySelector<HTMLElement>("#balance-card")!);
  loadAddress(el.querySelector<HTMLElement>("#deposit")!);
  loadPositions(el.querySelector<HTMLElement>("#positions")!);
}

async function loadAddress(el: HTMLElement) {
  try {
    const r = await fetch(`${BRIDGE}/address`);
    const j = await r.json();
    if (!j.ok) throw new Error(j.error || "address unavailable");
    el.textContent = j.address;
    el.addEventListener("click", () => {
      navigator.clipboard?.writeText(j.address).then(() => {
        const prev = el.textContent;
        el.textContent = "copied ✓";
        setTimeout(() => (el.textContent = prev), 900);
      });
    });
  } catch {
    el.textContent = "bridge offline";
  }
}

function maybeRenderPositions() {
  if (positionsEl && positionsData) renderPositions(positionsEl, positionsData);
}

async function loadOracle(card: HTMLElement) {
  try {
    const { priceUsd, pubkey } = await verifiedOraclePrice(ORACLE);
    price = Math.round(priceUsd);
    card.innerHTML = `
      <h2>Oracle price</h2>
      <div class="big">${fmtUsd(priceUsd)}<span class="u"> / BTC</span></div>
      <dl class="kv">
        <dt>verified</dt><dd style="color:var(--ok)">✓ digit signatures</dd>
        <dt>pubkey</dt><dd class="mono">${pubkey.slice(0, 16)}…</dd>
      </dl>`;
    maybeRenderPositions(); // CR depends on the price
  } catch (e: any) {
    card.innerHTML = `<h2>Oracle price</h2>
      <div class="banner bad"><b>Could not verify oracle</b>${e?.message ?? e}</div>`;
  }
}

const fmtBtc8 = (sats: number) => (sats / 1e8).toFixed(8);

async function loadBalance(card: HTMLElement) {
  const bal = card.querySelector<HTMLElement>("#bal")!;
  const satPending = card.querySelector<HTMLElement>("#sat-pending")!;
  const btc = card.querySelector<HTMLElement>("#btc")!;
  const btcPending = card.querySelector<HTMLElement>("#btc-pending")!;
  const sub = card.querySelector<HTMLElement>("#bal-sub")!;
  try {
    const r = await fetch(`${BRIDGE}/balance`);
    const j = await r.json();
    if (!j.ok) throw new Error(j.error || "balance unavailable");
    bal.textContent = fmtUsd((Number(j.display_micro ?? j.micro)) / 1e6);
    const notes: string[] = [];
    const minting = Number(j.pending_micro) || 0; // incoming mint
    const redeeming = Number(j.redeeming_micro) || 0; // outgoing redeem
    if (minting > 0) notes.push(`+${fmtUsd(minting / 1e6)} confirming`);
    if (redeeming > 0) notes.push(`−${fmtUsd(redeeming / 1e6)} redeeming`);
    satPending.textContent = notes.join("  ·  ");
    btc.innerHTML = `${fmtBtc8(Number(j.btc_sats) || 0)} <span class="u">BTC</span>`;
    const pend = Number(j.btc_pending_sats) || 0;
    btcPending.textContent = pend > 0 ? `+${fmtBtc8(pend)} pending` : "";
    sub.textContent = "held by your node on signet";
  } catch {
    bal.textContent = "—";
    btc.textContent = "—";
    sub.innerHTML = `bridge offline — run <span class="mono">npm run bridge</span>`;
  }
}

async function loadPositions(el: HTMLElement) {
  positionsEl = el;
  try {
    const r = await fetch(`${BRIDGE}/vaults`);
    const j = await r.json();
    positionsData = j.ok ? (j.vaults as Vault[]) : [];
  } catch {
    positionsData = [];
  }
  maybeRenderPositions();
}

function renderPositions(el: HTMLElement, vaults: Vault[]) {
  if (!vaults.length) {
    el.innerHTML = `<p class="hint">No open vaults yet. <a class="link" href="#birth">Mint SatUSD</a> against your BTC.</p>`;
    return;
  }
  el.innerHTML = vaults
    .map((v) => {
      const mintMicro = BigInt(v.mint_micro_usd);
      const terms = DEFAULT_TERMS(BigInt(v.collateral_sats), mintMicro);
      const cr = price ? crBps(BigInt(v.collateral_sats), mintMicro, price) : 0;
      const liqPrice = crashPriceCeiling(terms);
      const cls = !price ? "" : cr <= terms.liqCrBps ? "bad" : cr < terms.openingCrBps ? "warn" : "ok";
      const crTxt = price ? (cr / 100).toFixed(1) + "%" : "—";
      return `<div class="pos">
        <div class="pos-top">
          <span class="pos-amt">${fmtUsd(Number(mintMicro) / 1e6)}</span>
          <span class="pos-cr ${cls}">${crTxt}</span>
        </div>
        <dl class="kv">
          <dt>collateral</dt><dd>${fmtBtc(v.collateral_sats)} BTC</dd>
          <dt>liquidates at</dt><dd>${fmtUsd(liqPrice)}/BTC</dd>
          <dt>vault_id</dt><dd class="mono">${v.vault_id.slice(0, 16)}…</dd>
        </dl>
      </div>`;
    })
    .join("");
}
