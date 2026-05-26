// Cross-check the M4 StateCommit circuit against the `state_commit` vector (Rust +
// TS already agree) → 3-way. The circuit folds the canonical-encoding limbs to
// state_root_hash.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { createRequire } from "node:module";

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const wcBuilder = require(join(here, "build/m4_state_commit_js/witness_calculator.js"));
const wasm = readFileSync(join(here, "build/m4_state_commit_js/m4_state_commit.wasm"));
const wc = await wcBuilder(wasm);

const doc = JSON.parse(readFileSync(join(here, "../integration/vectors/vectors.json"), "utf8"));
const vectors = doc.vectors.filter((v) => v.op === "state_commit");

let pass = 0;
const failures = [];
for (const v of vectors) {
  const limbs = v.inputs.limbs.map((l) => BigInt("0x" + l).toString());
  const w = await wc.calculateWitness({ limbs }, true); // [1, commit, ...]
  if (BigInt(w[1]) === BigInt("0x" + v.output)) pass++;
  else failures.push(`${v.name}: circuit ${BigInt(w[1]).toString(16)} != ${v.output}`);
}

if (failures.length) {
  console.error("FAIL:\n" + failures.join("\n"));
  process.exit(1);
}
console.log(`circuit ↔ vectors: ${pass} state_commit vector(s) match (state_root_hash)`);
