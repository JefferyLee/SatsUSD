// Dump the m7_transition circom input (from the shared vector) to build/m7_input.json,
// so snarkjs can generate a real witness → proof for the monolithic transition.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const doc = JSON.parse(readFileSync(join(here, "../integration/vectors/vectors.json"), "utf8"));
const v = doc.vectors.find((x) => x.op === "m7_transition");
const i = v.inputs;

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

const input = {
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
};

mkdirSync(join(here, "build"), { recursive: true });
writeFileSync(join(here, "build/m7_input.json"), JSON.stringify(input));
console.log("wrote build/m7_input.json (m7_transition witness input)");
