// Cross-check the batch-root circuit against the shared `batch_root` vectors
// (Rust + TS already agree on them) → 3-way Rust = TS = circuit. Requires
// build/m4a_batch_root_js (run a circom --wasm compile first; see check.sh).
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { createRequire } from "node:module";

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const wcBuilder = require(join(here, "build/m4a_batch_root_js/witness_calculator.js"));
const wasm = readFileSync(join(here, "build/m4a_batch_root_js/m4a_batch_root.wasm"));
const wc = await wcBuilder(wasm);

const doc = JSON.parse(readFileSync(join(here, "../integration/vectors/vectors.json"), "utf8"));
const vectors = doc.vectors.filter((v) => v.op === "batch_root");

let pass = 0;
const failures = [];
for (const v of vectors) {
  const leaves = v.inputs.leaves.map((h) => BigInt("0x" + h).toString());
  const w = await wc.calculateWitness({ leaves }, true); // [1, root, ...]
  const got = BigInt(w[1]);
  const want = BigInt("0x" + v.output);
  if (got === want) pass++;
  else failures.push(`${v.name}: circuit ${got.toString(16)} != vector ${v.output}`);
}

if (failures.length) {
  console.error("FAIL:\n" + failures.join("\n"));
  process.exit(1);
}
console.log(`circuit ↔ vectors: ${pass} batch_root vectors match`);
