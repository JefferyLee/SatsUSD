// Cross-check the M4b LockInsert circuit against the shared `smt_insert` vectors
// (Rust + TS already agree) → 3-way. The circuit re-derives both the pre-insert
// (non-membership) and post-insert roots from one sibling path.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { createRequire } from "node:module";

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const wcBuilder = require(join(here, "build/m4b_lock_insert_js/witness_calculator.js"));
const wasm = readFileSync(join(here, "build/m4b_lock_insert_js/m4b_lock_insert.wasm"));
const wc = await wcBuilder(wasm);

const doc = JSON.parse(readFileSync(join(here, "../integration/vectors/vectors.json"), "utf8"));
const vectors = doc.vectors.filter((v) => v.op === "smt_insert");

let pass = 0;
const failures = [];
for (const v of vectors) {
  const key = v.inputs.key;
  const leaf = BigInt("0x" + v.inputs.leaf).toString();
  const siblings = v.inputs.siblings.map((s) => BigInt("0x" + s).toString());
  const pathBits = [];
  for (let d = 0; d < 256; d++) {
    const byte = parseInt(key.slice((d >> 3) * 2, (d >> 3) * 2 + 2), 16);
    pathBits.push(String((byte >> (7 - (d % 8))) & 1));
  }
  // Witness layout: [1, oldRoot, newRoot, ...].
  const w = await wc.calculateWitness({ leaf, pathBits, siblings }, true);
  const okOld = BigInt(w[1]) === BigInt("0x" + v.old_root);
  const okNew = BigInt(w[2]) === BigInt("0x" + v.new_root);
  if (okOld && okNew) pass++;
  else failures.push(`${v.name}: old=${okOld} new=${okNew}`);
}

if (failures.length) {
  console.error("FAIL:\n" + failures.join("\n"));
  process.exit(1);
}
console.log(`circuit ↔ vectors: ${pass} smt_insert vectors match (old + new root)`);
