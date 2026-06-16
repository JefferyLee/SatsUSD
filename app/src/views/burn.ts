import { verifiedQuote, verifiedOraclePrice } from "@satusd/sdk";

const LP = location.origin + "/lp";
const ORACLE = location.origin + "/oracle";
// A P2WPKH (bech32) payout: its asset-exclusion proof needs no taproot
// internal key, so the redeem broadcasts cleanly. An *external* P2TR (tb1p)
// payout fails — tapd can't supply an internal key for a foreign taproot
// output's exclusion proof (you can redeem to a taproot address you control).
const DEMO_PAYOUT = "tb1q39m56ht7sjauk63uhtp6eu23jytm600jpn2h2a";

// The exact checks verifiedQuote() runs before it returns. If it returns at
// all, every one of these passed (refusal-on-mismatch is the API contract),
// so we render them as confirmed — the phone did this math, not the LP.
const BATTERY = [
  ["rail_id", "Manifest re-hashed locally → rail_id (never trusts the LP's id)"],
  ["commitment", "SwapPlan recomputed → lock_template_commitment matches the quote"],
  ["lp_sig", "LP's BIP-340 signature over the canonical quote payload verified"],
  ["oracle_sig", "Every oracle attestation digit signature BIP-340-verified"],
  ["deviation", "Quoted price within the rail's own deviation bound vs the oracle"],
];

function fmtUsd(n: number) {
  return "$" + n.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
}

export function renderBurn(el: HTMLElement) {
  el.innerHTML = `
    <div class="card" id="oracle-card">
      <h2>Oracle price <span class="spin"></span></h2>
      <p class="hint">Signature-verified on this device against the oracle pubkey.</p>
    </div>
    <div class="card">
      <h2>Burn → redeem SatUSD for BTC</h2>
      <p class="hint">Fetch a real quote from the live signet LP and verify the whole thing
        locally before anything is signed.</p>
      <label for="amt">Amount (USD)</label>
      <input id="amt" type="number" inputmode="decimal" min="0.25" step="0.25" value="1.00" />
      <label for="addr">Payout BTC address (signet)</label>
      <input id="addr" class="mono" value="${DEMO_PAYOUT}" />
      <button id="go">Verify quote</button>
    </div>
    <div id="result"></div>
  `;

  loadOracle(el.querySelector("#oracle-card")!);

  const btn = el.querySelector<HTMLButtonElement>("#go")!;
  btn.addEventListener("click", () => runVerify(el, btn));
}

async function loadOracle(card: HTMLElement) {
  try {
    const { priceUsd, pubkey, eventTs } = await verifiedOraclePrice(ORACLE);
    card.innerHTML = `
      <h2>Oracle price</h2>
      <div class="big">${fmtUsd(priceUsd)}<span class="u"> / BTC</span></div>
      <dl class="kv">
        <dt>verified</dt><dd style="color:var(--ok)">✓ digit signatures</dd>
        <dt>pubkey</dt><dd class="mono">${pubkey.slice(0, 16)}…</dd>
        <dt>event</dt><dd class="mono">${eventTs}</dd>
      </dl>`;
  } catch (e: any) {
    card.innerHTML = `<h2>Oracle price</h2>
      <div class="banner bad"><b>Could not verify oracle</b>${e.message ?? e}</div>`;
  }
}

const BRIDGE = location.origin + "/bridge";

// Only reachable after on-device verification passed: the user taps to let the
// node actually sign + broadcast the redemption via the signing bridge.
function wireSign(result: HTMLElement, usd: number, addr: string) {
  const sign = result.querySelector<HTMLButtonElement>("#sign");
  if (!sign) return;
  sign.addEventListener("click", async () => {
    sign.disabled = true;
    sign.textContent = "signing & broadcasting…";
    try {
      const r = await fetch(BRIDGE + "/redeem", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ amount_usd: usd, payout_address: addr }),
      });
      const j = await r.json();
      if (!r.ok || !j.txid) throw new Error(j.error ?? `bridge ${r.status}`);
      sign.outerHTML = `
        <div class="banner ok"><b>Broadcast — SatUSD burned, BTC on the way</b>
          The node signed the redemption the phone approved.</div>
        <div class="card"><dl class="kv">
          <dt>txid</dt><dd class="mono">${j.txid}</dd>
        </dl></div>
        <a class="walletcta" href="#home">→ Back to wallet</a>`;
    } catch (e: any) {
      sign.disabled = false;
      sign.textContent = "Sign & broadcast — node signs";
      const note = document.createElement("div");
      note.className = "banner bad";
      note.innerHTML = `<b>Broadcast failed</b>${String(e?.message ?? e)}`;
      sign.before(note);
    }
  });
}

async function runVerify(el: HTMLElement, btn: HTMLButtonElement) {
  const result = el.querySelector<HTMLElement>("#result")!;
  const usd = parseFloat((el.querySelector<HTMLInputElement>("#amt")!).value || "0");
  const addr = (el.querySelector<HTMLInputElement>("#addr")!).value.trim();
  const micro = BigInt(Math.round(usd * 1_000_000));

  btn.disabled = true;
  result.innerHTML = `<div class="card"><span class="spin"></span> requesting & verifying quote…</div>`;

  try {
    const v = await verifiedQuote(LP, micro, addr, ORACLE);
    const quotedUsd = Number(100_000_000_000n / BigInt(v.quote.price_msat_per_usd));
    result.innerHTML = `
      <div class="banner ok"><b>Quote verified — safe to redeem</b>
        The phone checked all five proofs below independently. The node only signs next.</div>
      <div class="card">
        <div class="big">${v.userSats.toLocaleString()} <span class="u">sats</span></div>
        <p class="hint">payout for ${fmtUsd(usd)} of SatUSD</p>
        <dl class="kv">
          <dt>oracle price</dt><dd>${fmtUsd(v.oraclePriceUsd)}</dd>
          <dt>quoted price</dt><dd>${fmtUsd(quotedUsd)}</dd>
          <dt>deviation</dt><dd>${v.deviationBps.toFixed(1)} bps</dd>
          <dt>rail_id</dt><dd class="mono">${v.railId.slice(0, 16)}…</dd>
        </dl>
      </div>
      <div class="card">
        <h2>Verification battery</h2>
        <ul class="checks">
          ${BATTERY.map(
            ([, desc]) => `<li class="pass"><span class="mark">✓</span>
              <span class="what">${desc}</span></li>`,
          ).join("")}
        </ul>
      </div>
      <div class="card">
        <h2>Cryptographic material</h2>
        <dl class="kv">
          <dt>LP pubkey</dt><dd class="mono">${(v.raw.lp_pubkey ?? "").slice(0, 24)}…</dd>
          <dt>LP signature</dt><dd class="mono">${(v.raw.lp_sig ?? "").slice(0, 24)}…</dd>
          <dt>lock commitment</dt><dd class="mono">${(v.quote.lock_template_commitment ?? "").slice(0, 24)}…</dd>
        </dl>
      </div>
      <button class="secondary" id="sign">Sign &amp; broadcast — node signs</button>
    `;
    wireSign(result, usd, addr);
  } catch (e: any) {
    const msg = String(e?.message ?? e);
    // The SDK marks a verification refusal with "REFUSING"; anything else is an
    // operational/network error (LP couldn't quote, oracle unreachable). These
    // must read differently — a refusal is the safety property doing its job,
    // an operational error is just "try again".
    const refused = /REFUS/i.test(msg);
    result.innerHTML = refused
      ? `<div class="banner bad"><b>REFUSED — a proof did not check out</b>${msg}</div>
         <div class="card"><p class="hint">Refusal is the contract: if any proof fails, the phone
           will not let the node sign. This is the safety property, not an error.</p></div>`
      : `<div class="banner bad"><b>Couldn't get a quote</b>${msg}</div>
         <div class="card"><p class="hint">This is an LP/network issue, not a verification failure —
           nothing was proven false, the quote just couldn't be fetched. Try again.</p></div>`;
  } finally {
    btn.disabled = false;
  }
}
