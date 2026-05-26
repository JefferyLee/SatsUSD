// Cross-check the M4c OracleEddsa circuit against the `oracle_eddsa` vector. The
// circuit recomputes oracle_message_hash from the fields and verifies the
// EdDSA-BabyJub signature; circomlibjs unpacks the pinned pubkey/sig to coords.
// Rust (babyjubjub-rs) + this circuit (circomlib EdDSAPoseidonVerifier) are two
// independent EdDSA implementations agreeing on the same signature.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { createRequire } from "node:module";
import { buildEddsa } from "circomlibjs";

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const wcBuilder = require(join(here, "build/m4c_oracle_eddsa_js/witness_calculator.js"));
const wasm = readFileSync(join(here, "build/m4c_oracle_eddsa_js/m4c_oracle_eddsa.wasm"));
const wc = await wcBuilder(wasm);
const eddsa = await buildEddsa();
const F = eddsa.F;

const doc = JSON.parse(readFileSync(join(here, "../integration/vectors/vectors.json"), "utf8"));
const vectors = doc.vectors.filter((v) => v.op === "oracle_eddsa");

const hexToBuf = (h) => Uint8Array.from(h.match(/../g).map((b) => parseInt(b, 16)));

let pass = 0;
const failures = [];
for (const v of vectors) {
  const i = v.inputs;
  const A = eddsa.babyJub.unpackPoint(hexToBuf(i.pubkey));
  const sig = eddsa.unpackSignature(hexToBuf(i.sig));
  const input = {
    oracleSetEpoch: i.oracle_set_epoch,
    priceEpoch: i.price_epoch,
    timestampMs: i.timestamp_ms,
    priceE8: i.price_e8,
    Ax: F.toObject(A[0]).toString(),
    Ay: F.toObject(A[1]).toString(),
    R8x: F.toObject(sig.R8[0]).toString(),
    R8y: F.toObject(sig.R8[1]).toString(),
    S: sig.S.toString(),
  };
  try {
    // Witness gen succeeds only if EdDSAPoseidonVerifier's constraints hold.
    const w = await wc.calculateWitness(input, true); // [1, msg, ...]
    const msgOk = BigInt(w[1]) === BigInt("0x" + v.output);
    // Tampering the signature scalar must make witness generation fail.
    let tamperRejected = false;
    try {
      await wc.calculateWitness({ ...input, S: (BigInt(input.S) + 1n).toString() }, true);
    } catch {
      tamperRejected = true;
    }
    if (msgOk && tamperRejected) pass++;
    else failures.push(`${v.name}: msgOk=${msgOk} tamperRejected=${tamperRejected}`);
  } catch (e) {
    failures.push(`${v.name}: valid sig rejected — ${e}`);
  }
}

if (failures.length) {
  console.error("FAIL:\n" + failures.join("\n"));
  process.exit(1);
}
console.log(`circuit ↔ vectors: ${pass} oracle_eddsa attestation(s) verify (hash + sig)`);
