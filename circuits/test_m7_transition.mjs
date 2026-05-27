// Cross-check the M7 monolithic transition circuit against the `m7_transition`
// vector (Rust + TS already agree on the field commits) → 3-way. A successful
// witness calculation proves every binding constraint holds (lock_record
// membership, consumed/nullifier insert old→new, supply/epoch linkage), and the
// circuit's prevCommit/newCommit outputs must equal the vector's.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { createRequire } from "node:module";

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const wcBuilder = require(join(here, "build/m7_transition_js/witness_calculator.js"));
const wasm = readFileSync(join(here, "build/m7_transition_js/m7_transition.wasm"));
const wc = await wcBuilder(wasm);

const doc = JSON.parse(readFileSync(join(here, "../integration/vectors/vectors.json"), "utf8"));
const vectors = doc.vectors.filter((v) => v.op === "m7_transition");

const fr = (h) => BigInt("0x" + h).toString();
const frs = (a) => a.map(fr);
const bits = (keyHex) => {
  const out = [];
  for (let d = 0; d < 256; d++) {
    const byte = parseInt(keyHex.slice((d >> 3) * 2, (d >> 3) * 2 + 2), 16);
    out.push(String((byte >> (7 - (d % 8))) & 1));
  }
  return out;
};

let pass = 0;
const failures = [];
for (const v of vectors) {
  const i = v.inputs;
  // calculateWitness throws if any binding constraint is unsatisfiable.
  const w = await wc.calculateWitness(
    {
      prevFields: frs(i.prev_fields),
      newFields: frs(i.new_fields),
      lockLeaf: fr(i.lock_leaf),
      lockPathBits: bits(i.lock_key),
      lockMembershipSiblings: frs(i.lock_membership_siblings),
      consumedSiblings: frs(i.consumed_siblings),
      nfLeaf: fr(i.nf_leaf),
      nfPathBits: bits(i.nf_key),
      nfSiblings: frs(i.nf_siblings),
      amount: i.amount,
    },
    true,
  );
  // Witness layout: [1, prevCommit, newCommit, ...].
  const okPrev = BigInt(w[1]) === BigInt("0x" + v.prev_commit);
  const okNew = BigInt(w[2]) === BigInt("0x" + v.new_commit);
  if (okPrev && okNew) pass++;
  else failures.push(`${v.name}: prev ${BigInt(w[1]).toString(16)}/${v.prev_commit} new ${BigInt(w[2]).toString(16)}/${v.new_commit}`);
}

if (failures.length) {
  console.error("FAIL:\n" + failures.join("\n"));
  process.exit(1);
}
console.log(`circuit ↔ vectors: ${pass} m7_transition(s) match (prev/new commit + all bindings satisfiable)`);
