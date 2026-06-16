// Transfer = moving SatUSD on L1 (a Taproot Asset transfer anchored in Bitcoin).
// The phone independently verifies the asset's whole lineage — every step's
// asset commitment reconstructed (DL-23) and its anchor tx proven in a block —
// using the TS port of tapd's proof verifier. Nothing is trusted from a server;
// the proof file could come from anywhere, the phone checks the cryptography.
import {
  parseProofFile, parseProof, verifyLineage, verifyAnchor,
  parseAssetLeaf, assetId, proofGet, tlv,
} from "@satusd/sdk/taproof";

const PROOF_URL = "/demo-transfer.hex";

function hexToBytes(h: string): Uint8Array {
  const s = h.trim();
  const out = new Uint8Array(s.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(s.substr(i * 2, 2), 16);
  return out;
}
function hx(b: Uint8Array): string {
  let s = "";
  for (const x of b) s += x.toString(16).padStart(2, "0");
  return s;
}
// Bitcoin displays txids in reversed byte order vs the internal hash.
const txidDisplay = (b: Uint8Array) => hx(b.slice().reverse());

export function renderTransfer(el: HTMLElement) {
  el.innerHTML = `
    <div class="card">
      <h2>Verify a real transfer</h2>
      <p class="hint">This is a real SatUSD transfer that already happened on signet. The
        phone re-verifies its entire lineage on-device — each hop's asset commitment + its
        anchor in a Bitcoin block — trusting nothing but the cryptography.</p>
      <button id="go">Verify this transfer</button>
    </div>
    <div id="result"></div>
  `;
  const btn = el.querySelector<HTMLButtonElement>("#go")!;
  const run = () => verify(el, btn);
  btn.addEventListener("click", run);
  run(); // auto-verify on open
}

async function verify(el: HTMLElement, btn: HTMLButtonElement) {
  const result = el.querySelector<HTMLElement>("#result")!;
  btn.disabled = true;
  result.innerHTML = `<div class="card"><span class="spin"></span> loading proof & verifying lineage…</div>`;
  try {
    const data = hexToBytes(await (await fetch(PROOF_URL)).text());
    const pf = parseProofFile(data); // sha256 hash chain checked here
    const proofs = pf.proofs.map(parseProof);
    const head = verifyLineage(proofs); // DL-23 inclusion + lineage continuity

    const genesisLeaf = parseAssetLeaf(proofGet(proofs[0], tlv.ASSET_LEAF)!);
    const headLeaf = parseAssetLeaf(proofGet(proofs[proofs.length - 1], tlv.ASSET_LEAF)!);
    const aid = hx(assetId(headLeaf.genesis));
    const usd = Number(headLeaf.amount) / 1_000_000;
    const steps = proofs.map((p, i) => {
      const a = verifyAnchor(p);
      return { i, txid: txidDisplay(a.txid), vout: a.outputIndex };
    });
    // The asset's owner key (script key) at each end of the chain.
    const fromKey = hx(genesisLeaf.scriptKey);
    const toKey = hx(headLeaf.scriptKey);
    const fromTx = steps[0].txid;
    const toTx = steps[steps.length - 1].txid;
    const hops = proofs.length - 1; // transfers after genesis

    result.innerHTML = `
      <div class="banner ok"><b>Verified on-device — ${hops} transfer${hops === 1 ? "" : "s"} since issuance</b>
        Every hop's asset commitment was reconstructed and its anchor proven in a block.
        No server was trusted.</div>
      <div class="card">
        <div class="big">$${usd.toLocaleString(undefined, { minimumFractionDigits: 2 })} <span class="u">SatUSD</span></div>
        <p class="hint">asset_id ${aid.slice(0, 20)}… · ${headLeaf.groupKey ? "SatUSD group ✓" : "ungrouped"}</p>
      </div>
      <div class="card">
        <h2>What you're verifying</h2>
        <dl class="kv">
          <dt>from (issued to)</dt><dd class="mono">${fromKey.slice(0, 16)}…</dd>
          <dt>at genesis tx</dt><dd class="mono">${fromTx.slice(0, 20)}…</dd>
          <dt>to (now held by)</dt><dd class="mono">${toKey.slice(0, 16)}…</dd>
          <dt>at latest tx</dt><dd class="mono">${toTx.slice(0, 20)}…</dd>
        </dl>
        <p class="hint">${hops === 0
          ? "Issued, not yet transferred — verifying the genesis commitment."
          : `The asset moved from its issuance owner to the current holder over ${hops} on-chain transfer${hops === 1 ? "" : "s"}; each is checked below.`}</p>
      </div>
      <div class="card">
        <h2>Lineage (genesis → current)</h2>
        <ul class="checks">
          ${steps.map((s) => `<li class="pass"><span class="mark">✓</span>
            <span class="what">${s.i === 0 ? "genesis" : "transfer " + s.i}
              <small>${s.txid.slice(0, 32)}…:${s.vout}</small></span></li>`).join("")}
        </ul>
      </div>
      <div class="card"><p class="hint">The phone reconstructed each Taproot Asset
        commitment (MS-SMT → taproot tweak) and folded each anchor tx to its block
        header's merkle root. Refusal on any mismatch is the contract.</p></div>
    `;
  } catch (e: any) {
    const msg = String(e?.kind ?? e?.message ?? e);
    result.innerHTML = `
      <div class="banner bad"><b>REFUSED — lineage did not verify</b>${msg}</div>
      <div class="card"><p class="hint">A proof step failed verification — the phone
        will not accept this transfer as real. This is the safety property.</p></div>`;
  } finally {
    btn.disabled = false;
  }
}
