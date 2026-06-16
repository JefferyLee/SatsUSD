// Birth = minting a vault (spec 06 / CDP / J1): lock your own BTC, mint
// SatUSD against it. The phone computes the vault's terms on-device from
// the live oracle price — collateral ratio, the crash threshold, what a
// liquidation pays out — and REFUSES an under-collateralised open, before
// any node touches the chain. The node-side open+mint is the next
// increment (the vault itself is implemented + devnet-validated:
// crates/satusd-vault, spec 06).
import { verifiedOraclePrice } from "@satusd/sdk";
import {
  crBps, openingOk, crashPriceCeiling, crashPayout, vaultId,
  DEFAULT_TERMS, DEFAULT_PARAMS,
} from "@satusd/sdk/vault";

const ORACLE = location.origin + "/oracle";
const BRIDGE = location.origin + "/bridge";

const fmtUsd = (n: number) =>
  "$" + n.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
const fmtSats = (n: bigint) => Number(n).toLocaleString();

export function renderBirth(el: HTMLElement) {
  el.innerHTML = `
    <div class="card" id="oracle-card">
      <h2>Oracle price <span class="spin"></span></h2>
      <p class="hint">Signature-verified on this device.</p>
    </div>
    <div class="card">
      <h2>Birth — mint SatUSD from your own BTC</h2>
      <p class="hint">Lock BTC in a DLC vault and mint SatUSD against it — a position
        you alone control, no issuer, no custody. The phone checks the terms first.</p>
      <label for="coll">Collateral (BTC)</label>
      <input id="coll" type="number" inputmode="decimal" min="0" step="0.001" value="0.02343750" />
      <label for="mint">Mint (USD)</label>
      <input id="mint" type="number" inputmode="decimal" min="1" step="1" value="1000" />
      <button id="go">Preview vault</button>
    </div>
    <div id="result"></div>
  `;
  loadOracle(el.querySelector("#oracle-card")!);
  const btn = el.querySelector<HTMLButtonElement>("#go")!;
  btn.addEventListener("click", () => preview(el));
}

let lastPrice = 0;

async function loadOracle(card: HTMLElement) {
  try {
    const { priceUsd, pubkey } = await verifiedOraclePrice(ORACLE);
    lastPrice = Math.round(priceUsd);
    card.innerHTML = `
      <h2>Oracle price</h2>
      <div class="big">${fmtUsd(priceUsd)}<span class="u"> / BTC</span></div>
      <dl class="kv">
        <dt>verified</dt><dd style="color:var(--ok)">✓ digit signatures</dd>
        <dt>pubkey</dt><dd class="mono">${pubkey.slice(0, 16)}…</dd>
      </dl>`;
  } catch (e: any) {
    card.innerHTML = `<h2>Oracle price</h2>
      <div class="banner bad"><b>Could not verify oracle</b>${e?.message ?? e}</div>`;
  }
}

function preview(el: HTMLElement) {
  const result = el.querySelector<HTMLElement>("#result")!;
  const coll = parseFloat((el.querySelector<HTMLInputElement>("#coll")!).value || "0");
  const usd = parseFloat((el.querySelector<HTMLInputElement>("#mint")!).value || "0");
  const price = lastPrice;
  if (!price) {
    result.innerHTML = `<div class="banner bad"><b>No oracle price yet</b>Wait for the verified price above.</div>`;
    return;
  }

  const collateralSats = BigInt(Math.round(coll * 1e8));
  const mintMicro = BigInt(Math.round(usd * 1_000_000));
  const terms = DEFAULT_TERMS(collateralSats, mintMicro);
  const cr = crBps(collateralSats, mintMicro, price);

  if (!openingOk(terms, price)) {
    const needSats = Math.ceil((usd * (terms.openingCrBps / 10_000)) / price * 1e8);
    result.innerHTML = `
      <div class="banner bad"><b>REFUSED — under-collateralised</b>
        CR ${(cr / 100).toFixed(1)}% is below the ${(terms.openingCrBps / 100).toFixed(0)}% opening
        minimum. Lock ≥ ${(needSats / 1e8).toFixed(8)} BTC for ${fmtUsd(usd)} at ${fmtUsd(price)}.</div>
      <div class="card"><p class="hint">Refusal is the contract: the phone won't let the
        node open an unsafe vault.</p></div>`;
    return;
  }

  // Liquidation snapshot: the payout if the price falls to the crash threshold.
  const ceiling = crashPriceCeiling(terms);
  const po = crashPayout(collateralSats, mintMicro, ceiling, DEFAULT_PARAMS);
  const vid = vaultId(terms);

  result.innerHTML = `
    <div class="banner ok"><b>Vault terms verified — safe to open</b>
      The phone computed these from the signed price. The node would open + mint next.</div>
    <div class="card">
      <div class="big">${fmtUsd(usd)} <span class="u">SatUSD minted</span></div>
      <p class="hint">backed by ${coll.toFixed(8)} BTC you alone control</p>
      <dl class="kv">
        <dt>collateral ratio</dt><dd>${(cr / 100).toFixed(1)}%</dd>
        <dt>opening minimum</dt><dd>${(terms.openingCrBps / 100).toFixed(0)}%</dd>
        <dt>liquidation at</dt><dd>${(terms.liqCrBps / 100).toFixed(0)}% · ${fmtUsd(ceiling)}/BTC</dd>
        <dt>vault_id</dt><dd class="mono">${vid.slice(0, 16)}…</dd>
      </dl>
    </div>
    <div class="card">
      <h2>If BTC crashes to ${fmtUsd(ceiling)}</h2>
      <p class="hint">A crash CET becomes broadcastable by anyone; the collateral splits:</p>
      <dl class="kv">
        <dt>→ reserve (backs your SatUSD)</dt><dd>${fmtSats(po.reserveSats)} sats</dd>
        <dt>→ liquidator bounty</dt><dd>${fmtSats(po.broadcasterSats)} sats</dd>
        <dt>→ back to you</dt><dd>${fmtSats(po.minterSats)} sats</dd>
      </dl>
    </div>
    <div class="card">
      <h2>Verification battery</h2>
      <ul class="checks">
        <li class="pass"><span class="mark">✓</span><span class="what">Collateral ratio recomputed from the signed oracle price</span></li>
        <li class="pass"><span class="mark">✓</span><span class="what">Opening CR met (refusal-on-fail is the contract)</span></li>
        <li class="pass"><span class="mark">✓</span><span class="what">Crash threshold + liquidation payout derived on-device</span></li>
        <li class="pass"><span class="mark">✓</span><span class="what">vault_id committed to the exact terms</span></li>
      </ul>
    </div>
    <button class="secondary" id="open">Open &amp; mint — node opens the vault</button>
    <div class="card"><p class="hint">On tap, the node funds the MuSig2(minter, reserve) output Q with
      your BTC and reissues the SatUSD into the canonical group on <b>signet</b> — the same group
      your balance reads. The phone verified the terms above; the node signs.</p></div>
    <div id="node-result"></div>
  `;

  const openBtn = result.querySelector<HTMLButtonElement>("#open");
  openBtn?.addEventListener("click", () => openVault(result, collateralSats, usd, openBtn));
}

async function openVault(result: HTMLElement, collateralSats: bigint, usd: number, btn: HTMLButtonElement) {
  const node = result.querySelector<HTMLElement>("#node-result")!;
  btn.disabled = true;
  btn.textContent = "Opening on devnet… node is signing";
  node.innerHTML = `<div class="card"><p class="hint">Node funding Q + reissuing SatUSD on the devnet…</p></div>`;
  try {
    const r = await fetch(`${BRIDGE}/vault/open`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ collateral_sats: Number(collateralSats), mint_usd: usd }),
    });
    const j = await r.json();
    if (!j.ok) throw new Error(j.error || "vault-open failed");
    const minted = Number(j.mint_micro_usd) / 1e6;
    const net = j.net === "signet" ? "signet" : "devnet";
    const banner = j.pending
      ? `<div class="banner ok"><b>Broadcast on signet — collateral locked, SatUSD minting</b>
          Both txs are in the mempool; they confirm in a few minutes, then the mint lands in your balance.</div>`
      : `<div class="banner ok"><b>Vault opened — collateral locked, SatUSD minted</b>
          The node funded Q and reissued into the SatUSD group.</div>`;
    const mintTxRow = j.mint_txid
      ? `<dt>mint txid</dt><dd class="mono">${String(j.mint_txid).slice(0, 20)}…</dd>`
      : "";
    const supplyRow = j.pending
      ? `<dt>reissue</dt><dd>pending confirmation</dd>`
      : `<dt>group supply</dt><dd>${fmtUsd(j.supply_before_micro / 1e6)} → ${fmtUsd(j.supply_after_micro / 1e6)}</dd>`;
    node.innerHTML = `
      ${banner}
      <div class="card">
        <dl class="kv">
          <dt>collateral locked at Q</dt><dd>${fmtSats(BigInt(j.collateral_sats))} sats</dd>
          <dt>funding output</dt><dd class="mono">${String(j.funding_txid).slice(0, 20)}…:${j.funding_vout}</dd>
          ${mintTxRow}
          <dt>vault_id</dt><dd class="mono">${String(j.vault_id).slice(0, 16)}…</dd>
          <dt>SatUSD minted</dt><dd>${fmtUsd(minted)}</dd>
          ${supplyRow}
        </dl>
        <p class="hint">Pure-BTC collateral at a 2-of-2 taproot output on ${net}; the SatUSD reissues into the canonical group, backed by your vault.</p>
      </div>
      <a class="walletcta" href="#home">→ View in your wallet</a>`;
    btn.textContent = j.pending ? "Broadcast ✓" : "Vault opened ✓";
  } catch (e: any) {
    node.innerHTML = `<div class="banner bad"><b>Open failed</b>${e?.message ?? e}</div>`;
    btn.disabled = false;
    btn.textContent = "Open & mint — retry";
  }
}
