// Cross-check the M4b SMT-fold circuit against the shared `smt` vectors (Rust + TS
// already agree on them) → 3-way Rust = TS = circuit. Requires build/m4b_smt_js
// (run a circom --wasm compile first; see check.sh).
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { createRequire } from "node:module";

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const wcBuilder = require(join(here, "build/m4b_smt_js/witness_calculator.js"));
const wasm = readFileSync(join(here, "build/m4b_smt_js/m4b_smt.wasm"));
const wc = await wcBuilder(wasm);

const doc = JSON.parse(readFileSync(join(here, "../integration/vectors/vectors.json"), "utf8"));
const vectors = doc.vectors.filter((v) => v.op === "smt");

let pass = 0;
const failures = [];
for (const v of vectors) {
  const key = v.inputs.key; // 32-byte hex
  const leaf = BigInt("0x" + v.inputs.leaf).toString();
  const siblings = v.inputs.siblings.map((s) => BigInt("0x" + s).toString());
  // Path bits MSB-first, matching satusd_crypto::smt::bit().
  const pathBits = [];
  for (let d = 0; d < 256; d++) {
    const byte = parseInt(key.slice((d >> 3) * 2, (d >> 3) * 2 + 2), 16);
    pathBits.push(String((byte >> (7 - (d % 8))) & 1));
  }
  const w = await wc.calculateWitness({ leaf, pathBits, siblings }, true); // [1, root, ...]
  const got = BigInt(w[1]);
  const want = BigInt("0x" + v.output);
  if (got === want) pass++;
  else failures.push(`${v.name}: circuit ${got.toString(16)} != vector ${v.output}`);
}

if (failures.length) {
  console.error("FAIL:\n" + failures.join("\n"));
  process.exit(1);
}
console.log(`circuit ↔ vectors: ${pass} smt vectors match (fold → root)`);
