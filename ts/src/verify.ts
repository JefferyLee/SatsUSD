// Cross-language verifier: reads the Rust-generated vectors and asserts the
// TypeScript reference produces byte-identical encodings and hashes.
// Run: node ts/src/verify.ts   (Node >= 23 strips types; no build/install needed)

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { Encoder, bytesToHex } from "./encoder.ts";
import { encodeByType, structHashes, deriveOutput, sha256Hex, domainTag } from "./satusd.ts";
import {
  poseidon2Hex,
  hashBytesHex,
  burnInternalKeyHex,
  burnTweakHex,
  sinkScriptKeyHex,
  tapTweakHex,
  numsKeyHex,
  collateralRatioPpm,
  recomputeTier,
  batchRootHex,
} from "./crypto.ts";

const here = dirname(fileURLToPath(import.meta.url));
const vectorsPath = join(here, "../../integration/vectors/vectors.json");

const doc = JSON.parse(readFileSync(vectorsPath, "utf8"));

let pass = 0;
const failures: string[] = [];

function check(name: string, got: string, want: string, what: string): void {
  if (got === want) {
    pass++;
  } else {
    failures.push(`${name} [${what}]: got ${got} want ${want}`);
  }
}

// Domain separator registry: name -> raw ASCII bytes (no padding).
for (const d of doc.domains as Array<{ name: string; tag_hex: string }>) {
  check(d.name, bytesToHex(domainTag(d.name)), d.tag_hex, "domain_tag");
}

for (const v of doc.vectors as any[]) {
  if (v.kind === "struct") {
    const enc = encodeByType(v.type, v.fields);
    const encHex = bytesToHex(enc);
    check(v.name, encHex, v.encoding_hex, "encoding");
    check(v.name, sha256Hex(enc), v.sha256_of_encoding, "sha256_of_encoding");
    const hashes = structHashes(v.type, v.fields);
    for (const [k, want] of Object.entries(v.hashes as Record<string, string>)) {
      check(v.name, hashes[k] ?? "<missing>", want, k);
    }
  } else if (v.kind === "derive") {
    check(v.name, deriveOutput(v.type, v.inputs), v.output, v.type);
  } else if (v.kind === "crypto") {
    switch (v.op) {
      case "poseidon2":
        check(v.name, poseidon2Hex(v.inputs.a, v.inputs.b), v.output, "poseidon2");
        break;
      case "hash_bytes":
        check(v.name, hashBytesHex(v.inputs.input), v.output, "hash_bytes");
        break;
      case "burn_sink":
        check(v.name, burnInternalKeyHex(v.inputs.asset_family_id), v.internal_key, "internal_key");
        check(v.name, burnTweakHex(v.inputs.asset_family_id), v.burn_tweak, "burn_tweak");
        check(v.name, sinkScriptKeyHex(v.inputs.asset_family_id), v.sink_script_key, "sink_script_key");
        break;
      case "tap_tweak":
        check(v.name, tapTweakHex(v.inputs.internal_key, v.inputs.tweak), v.output, "tap_tweak");
        break;
      case "nums_key":
        check(v.name, numsKeyHex(v.inputs.domain, v.inputs.salt), v.output, "nums_key");
        break;
      case "batch_root":
        check(v.name, batchRootHex(v.inputs.leaves), v.output, "batch_root");
        break;
      case "tier": {
        const r = BigInt(v.inputs.reserve_sats);
        const s = BigInt(v.inputs.supply_atoms);
        const p = BigInt(v.inputs.price_e8);
        const cr = collateralRatioPpm(r, s, p);
        check(v.name, cr === null ? "null" : cr.toString(), v.cr_ppm ?? "null", "cr_ppm");
        check(v.name, String(recomputeTier(r, s, p)), String(v.tier), "tier");
        break;
      }
      default:
        failures.push(`${v.name}: unknown crypto op ${v.op}`);
    }
  } else {
    failures.push(`${v.name}: unknown kind ${v.kind}`);
  }
}

const total = pass + failures.length;
console.log(`checked ${total} assertions across ${doc.count} vectors + ${doc.domains.length} domains`);
console.log(`pass: ${pass}, fail: ${failures.length}`);

if (failures.length > 0) {
  console.error("\nFAILURES (first 20):");
  for (const f of failures.slice(0, 20)) console.error("  " + f);
  process.exit(1);
}
console.log("OK — Rust and TypeScript byte-match.");

// Encoder is exercised above via encodeByType; explicit import keeps it linted as used.
void Encoder;
