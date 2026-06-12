// @satusd/sdk — the agent-facing client (MVP alpha).
//
// For an AI agent, "verifiable" is not a feature, it is the only
// usable interface. This SDK gives an agent the CLI's full
// verification battery as plain functions: every quote is checked —
// manifest → rail_id recomputed, SwapPlan → commitment recomputed,
// the LP's BIP-340 signature verified, the price checked against
// the public oracle's *signature-verified* attestation, inside the
// rail's own deviation bound. Refusal on mismatch is the API
// contract, not an error path.
//
// Alpha scope: verify + decide (J2 + quote-side of J3). Settlement
// execution (tapd/lnd signing) runs through the `satusd` CLI; a
// native execution path and an MCP server wrapper are the next
// increments.

import {
  taggedHash,
  encodeManifest,
  railIdHex,
  encodeQuote,
  swapPlanFromQuote,
  schnorrVerify,
  digits,
  priceFromDigits,
} from "./rail.ts";
import { hexToBytes, bytesToHex } from "./encoder.ts";

export { railIdHex, swapPlanFromQuote, schnorrVerify, encodeManifest, encodeQuote };

const DEFAULT_ORACLE = "http://207.148.98.132:9590";

async function getText(base: string, path: string): Promise<string> {
  const r = await fetch(base.startsWith("http") ? base + path : `http://${base}${path}`);
  if (!r.ok) throw new Error(`GET ${path}: ${r.status}`);
  return r.text();
}

async function postJson(base: string, path: string, body: unknown): Promise<any> {
  const r = await fetch(base.startsWith("http") ? base + path : `http://${base}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const j = await r.json();
  if (!r.ok) throw new Error(`POST ${path}: ${j.error ?? r.status}`);
  return j;
}

/// Latest oracle price with every digit signature BIP-340-verified
/// against the oracle pubkey — the agent-grade read.
export async function verifiedOraclePrice(
  oracle: string = DEFAULT_ORACLE,
  expectedPubkey?: string,
): Promise<{ priceUsd: number; eventTs: string; pubkey: string }> {
  const ts = (await getText(oracle, "/v0/latest")).trim();
  const tlvHex = (await getText(oracle, `/v0/attestation/${ts}`)).trim();
  const tlv = hexToBytes(tlvHex);
  // oracle_attestation TLV: skip bigsize type (3B for 55400) + bigsize len.
  // Parse strictly enough for verification: locate fields per Messaging.md.
  let off = 3; // 0xfd + u16 type
  if (tlv[off] === 0xfd) off += 3;
  else off += 1; // value length
  const idLen = tlv[off];
  off += 1;
  const eventId = new TextDecoder().decode(tlv.slice(off, off + idLen));
  off += idLen;
  const pubkey = bytesToHex(tlv.slice(off, off + 32));
  off += 32;
  if (expectedPubkey && pubkey !== expectedPubkey) {
    throw new Error(`oracle pubkey ${pubkey} != pinned ${expectedPubkey} — REFUSING`);
  }
  const count = (tlv[off] << 8) | tlv[off + 1];
  off += 2;
  const sigs: Uint8Array[] = [];
  for (let i = 0; i < count; i++) {
    sigs.push(tlv.slice(off, off + 64));
    off += 64;
  }
  const outcomes: number[] = [];
  for (let i = 0; i < count; i++) {
    const len = tlv[off];
    outcomes.push(tlv[off + 1] === 0x31 ? 1 : 0);
    off += 1 + len;
  }
  for (let i = 0; i < count; i++) {
    const msg = taggedHash("DLC/oracle/attestation/v0", new TextEncoder().encode(String(outcomes[i])));
    if (!schnorrVerify(pubkey, msg, sigs[i])) {
      throw new Error(`attestation digit ${i}: signature INVALID — REFUSING`);
    }
  }
  return { priceUsd: priceFromDigits(outcomes), eventTs: eventId.split("/").pop() ?? ts, pubkey };
}

export interface VerifiedQuote {
  quote: any;
  plan: any;
  railId: string;
  userSats: bigint;
  oraclePriceUsd: number;
  deviationBps: number;
  raw: any;
}

/// Fetch a quote and run the full battery. Throws (refuses) on any
/// mismatch; a returned object is a quote the agent may act on.
export async function verifiedQuote(
  lp: string,
  amountMicroUsd: bigint,
  payoutAddress: string,
  oracle: string = DEFAULT_ORACLE,
): Promise<VerifiedQuote> {
  const m = JSON.parse(await getText(lp, "/v0/manifest"));
  const manifest = m.manifest;
  const railId = railIdHex(manifest); // recomputed locally, never trusted
  const raw = await postJson(lp, "/v0/quote", {
    amount_micro_usd: amountMicroUsd.toString(),
    payout_address: payoutAddress,
  });

  const quote = {
    rail_id: railId,
    price_msat_per_usd: raw.quote.price_msat_per_usd,
    amount_micro_usd: amountMicroUsd.toString(),
    expiry_unix_s: raw.quote.expiry_unix_s,
    lock_template_commitment: raw.quote.lock_template_commitment,
  };
  const plan = swapPlanFromQuote(manifest, quote);
  if (plan.error) throw new Error(`plan: ${plan.error} — REFUSING`);
  if (plan.lock_template_commitment !== quote.lock_template_commitment) {
    throw new Error("lock_template_commitment mismatch — REFUSING");
  }
  // LP signature over the canonical payload.
  const payloadHex = bytesToHex(taggedHash("SatUSD/quote/v1", encodeQuote(quote)));
  if (payloadHex !== raw.signing_payload) throw new Error("signing_payload mismatch — REFUSING");
  if (!schnorrVerify(raw.lp_pubkey, hexToBytes(payloadHex), hexToBytes(raw.lp_sig))) {
    throw new Error("LP quote signature INVALID — REFUSING");
  }
  // Price vs the signature-verified oracle, inside the rail's bound.
  const { priceUsd } = await verifiedOraclePrice(oracle);
  const quotedUsd = Number(100_000_000_000n / BigInt(quote.price_msat_per_usd));
  const deviationBps = Math.abs(quotedUsd - priceUsd) * 10_000 / priceUsd;
  if (deviationBps > manifest.price_dev_bound_bps) {
    throw new Error(
      `price $${quotedUsd} deviates ${deviationBps.toFixed(1)} bps from oracle $${priceUsd} (bound ${manifest.price_dev_bound_bps}) — REFUSING`,
    );
  }
  return {
    quote,
    plan,
    railId,
    userSats: BigInt(plan.user_sats),
    oraclePriceUsd: priceUsd,
    deviationBps,
    raw,
  };
}

// keep tree-shaken helpers exported for advanced agents
export { digits, priceFromDigits };
