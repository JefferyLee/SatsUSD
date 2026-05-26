// Cross-check the M4b LockFinalize circuit against the `lock_finalize` vector
// (Rust + TS already agree) → 3-way. The circuit re-derives the lock_record
// membership root and the consumed / nullifier insert (old + new) roots.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { createRequire } from "node:module";

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const wcBuilder = require(join(here, "build/m4b_lock_finalize_js/witness_calculator.js"));
const wasm = readFileSync(join(here, "build/m4b_lock_finalize_js/m4b_lock_finalize.wasm"));
const wc = await wcBuilder(wasm);

const doc = JSON.parse(readFileSync(join(here, "../integration/vectors/vectors.json"), "utf8"));
const vectors = doc.vectors.filter((v) => v.op === "lock_finalize");

const bits = (keyHex) => {
  const out = [];
  for (let d = 0; d < 256; d++) {
    const byte = parseInt(keyHex.slice((d >> 3) * 2, (d >> 3) * 2 + 2), 16);
    out.push(String((byte >> (7 - (d % 8))) & 1));
  }
  return out;
};
const fr = (h) => BigInt("0x" + h).toString();
const frs = (a) => a.map(fr);

let pass = 0;
const failures = [];
for (const v of vectors) {
  const i = v.inputs;
  // Witness: [1, lockRecordRoot, consumedOld, consumedNew, nfOld, nfNew, ...].
  const w = await wc.calculateWitness(
    {
      lockLeaf: fr(i.lock_leaf),
      lockPathBits: bits(i.lock_key),
      lockMembershipSiblings: frs(i.lock_membership_siblings),
      consumedSiblings: frs(i.consumed_siblings),
      nfLeaf: fr(i.nf_leaf),
      nfPathBits: bits(i.nf_key),
      nfSiblings: frs(i.nf_siblings),
    },
    true,
  );
  const got = [w[1], w[2], w[3], w[4], w[5]].map((x) => BigInt(x));
  const want = [v.lock_record_root, v.consumed_old, v.consumed_new, v.nf_old, v.nf_new].map(
    (h) => BigInt("0x" + h),
  );
  if (got.every((g, j) => g === want[j])) pass++;
  else failures.push(`${v.name}: ${got.map((g) => g.toString(16))} != ${want.map((x) => x.toString(16))}`);
}

if (failures.length) {
  console.error("FAIL:\n" + failures.join("\n"));
  process.exit(1);
}
console.log(`circuit ↔ vectors: ${pass} lock_finalize transition(s) match (5 roots)`);
