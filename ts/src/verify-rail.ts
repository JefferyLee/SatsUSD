// Cross-language verifier for the rebuilt protocol surface: reads
// integration/vectors/rail-vectors.json (Rust-generated) and asserts
// the TypeScript reference in rail.ts reproduces every encoding,
// hash, signature, and derivation byte-for-byte.
// Run: node ts/src/verify-rail.ts

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { hexToBytes, bytesToHex } from "./encoder.ts";
import {
  taggedHash,
  bigsizeHex,
  encodeManifest,
  railIdHex,
  encodeAssetMeta,
  encodeQuote,
  quoteSigningPayloadHex,
  quoteUserSats,
  swapPlanFromQuote,
  capacitySats,
  computeAllotment,
  deriveBurnKeyHex,
  digits,
  priceFromDigits,
  oracleNonce,
  attestationMsg,
  oracleFromSeed,
  announce,
  attest,
  anticipationHolds,
  bucketOf,
  bucketSchedule,
  bucketAdaptorPointHex,
  bucketSecretHex,
  secretMatchesPoint,
  internalTwap,
  fundingOutput,
  siblingPreimageHex,
} from "./rail.ts";

const here = dirname(fileURLToPath(import.meta.url));
const doc = JSON.parse(
  readFileSync(join(here, "../../integration/vectors/rail-vectors.json"), "utf8"),
);

let pass = 0;
const failures: string[] = [];

function check(name: string, got: unknown, want: unknown, what: string): void {
  const g = got === null || got === undefined ? "null" : String(got);
  const w = want === null || want === undefined ? "null" : String(want);
  if (g === w) {
    pass++;
  } else {
    failures.push(`${name} [${what}]: got ${g} want ${w}`);
  }
}

for (const v of doc.vectors as any[]) {
  switch (v.kind) {
    case "tagged_hash":
      check(
        v.name,
        bytesToHex(taggedHash(v.inputs.tag, hexToBytes(v.inputs.msg))),
        v.output,
        "tagged_hash",
      );
      break;

    case "bigsize":
      check(v.name, bigsizeHex(v.inputs.value), v.output, "bigsize");
      break;

    case "rail_manifest":
      check(v.name, bytesToHex(encodeManifest(v.fields)), v.encoding_hex, "encoding");
      check(v.name, railIdHex(v.fields), v.rail_id, "rail_id");
      break;

    case "asset_meta":
      check(v.name, bytesToHex(encodeAssetMeta(v.fields)), v.encoding_hex, "encoding");
      break;

    case "quote":
      check(v.name, bytesToHex(encodeQuote(v.fields)), v.encoding_hex, "encoding");
      check(v.name, quoteSigningPayloadHex(v.fields), v.signing_payload, "signing_payload");
      check(v.name, quoteUserSats(v.fields).toString(), v.user_sats, "user_sats");
      break;

    case "swap_plan": {
      const got = swapPlanFromQuote(v.inputs.manifest, v.inputs.quote);
      if (v.result.error !== undefined) {
        check(v.name, got.error ?? "<plan>", v.result.error, "error");
      } else {
        for (const k of Object.keys(v.result)) {
          check(v.name, got[k], v.result[k], k);
        }
      }
      break;
    }

    case "capacity":
      check(
        v.name,
        capacitySats(
          BigInt(v.inputs.retained_fees_cum_sats),
          BigInt(v.inputs.lp_stake_sats),
        ).toString(),
        v.output,
        "capacity",
      );
      break;

    case "allotment": {
      const got = computeAllotment(v.inputs);
      check(v.name, got.input_hash, v.input_hash, "input_hash");
      check(v.name, got.encoding_hex, v.encoding_hex, "encoding");
      check(v.name, got.plan_hash, v.plan_hash, "plan_hash");
      check(v.name, got.unallocated_sats, v.unallocated_sats, "unallocated");
      break;
    }

    case "burn_key":
      check(
        v.name,
        deriveBurnKeyHex(
          v.inputs.txid_wire,
          v.inputs.vout,
          v.inputs.asset_id,
          v.inputs.script_key_xonly,
        ),
        v.output,
        "burn_key",
      );
      break;

    case "oracle_digits": {
      const ds = digits(v.inputs.price_usd);
      check(v.name, ds.join(""), (v.digits as number[]).join(""), "digits");
      check(v.name, priceFromDigits(ds), v.recomposed, "recomposed");
      break;
    }

    case "oracle_nonce":
      check(
        v.name,
        bytesToHex(oracleNonce(v.inputs.seed, v.inputs.event_id, v.inputs.digit_index)),
        v.output,
        "nonce",
      );
      break;

    case "attestation_msg":
      check(v.name, bytesToHex(attestationMsg(v.inputs.digit)), v.output, "msg");
      break;

    case "oracle_pair": {
      const o = oracleFromSeed(v.inputs.master_seed);
      check(v.name, o.pubkeyHex, v.pubkey, "pubkey");
      const ann = announce(o, v.inputs.unix_ts);
      check(v.name, ann.tlvHex, v.announcement_tlv_hex, "announcement_tlv");
      const att = attest(o, v.inputs.unix_ts, v.inputs.price_usd);
      check(v.name, att.tlvHex, v.attestation_tlv_hex, "attestation_tlv");
      // The DLC contract: every digit's anticipation point S = R + e·P
      // equals s·G of the published scalar.
      check(
        v.name,
        anticipationHolds(ann.noncePoints, o.pubkeyHex, att.signatures, v.inputs.price_usd),
        true,
        "anticipation",
      );
      break;
    }

    case "bucket_of":
      check(v.name, bucketOf(v.inputs.price_usd, v.inputs.m), v.output, "bucket_of");
      break;

    case "bucket_schedule": {
      const got = bucketSchedule(v.inputs.m, BigInt(v.inputs.user_sats_factor));
      check(v.name, JSON.stringify(got), JSON.stringify(v.buckets), "schedule");
      break;
    }

    case "bucket_point": {
      const o = oracleFromSeed(v.inputs.master_seed);
      const ann = announce(o, v.inputs.unix_ts);
      const att = attest(o, v.inputs.unix_ts, v.inputs.price_usd);
      const m = v.inputs.m;
      const win = bucketOf(v.inputs.price_usd, m);
      check(v.name, win, v.winning_bucket, "winning_bucket");
      const point = bucketAdaptorPointHex(ann.noncePoints, o.pubkeyHex, m, win);
      check(v.name, point, v.adaptor_point, "adaptor_point");
      check(
        v.name,
        bucketAdaptorPointHex(ann.noncePoints, o.pubkeyHex, m, (win + 1) % (1 << m)),
        v.other_bucket_point,
        "other_bucket_point",
      );
      const secret = bucketSecretHex(att.signatures, m);
      check(v.name, secret, v.bucket_secret, "bucket_secret");
      check(v.name, secretMatchesPoint(secret, point), true, "secret_matches_point");
      break;
    }

    case "internal_twap":
      check(
        v.name,
        internalTwap(v.inputs.points, v.inputs.min_volume_micro_usd),
        v.output,
        "marker",
      );
      break;

    case "funding_output": {
      const got = fundingOutput(
        v.inputs.internal_x,
        v.inputs.ta_leaf_hash,
        v.inputs.refund_script_hex,
      );
      check(v.name, got.merkleRoot, v.merkle_root, "merkle_root");
      check(v.name, got.outputX, v.output_x, "output_x");
      check(
        v.name,
        siblingPreimageHex(v.inputs.refund_script_hex),
        v.sibling_preimage_hex,
        "sibling_preimage",
      );
      break;
    }

    default:
      failures.push(`${v.name}: unknown kind ${v.kind}`);
  }
}

const total = pass + failures.length;
console.log(`checked ${total} assertions across ${doc.count} rail vectors`);
console.log(`pass: ${pass}, fail: ${failures.length}`);

if (failures.length > 0) {
  console.error("\nFAILURES (first 20):");
  for (const f of failures.slice(0, 20)) console.error("  " + f);
  process.exit(1);
}
console.log("OK — Rust and TypeScript byte-match on the rail surface.");
