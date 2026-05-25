// Cross-check the M4a circuit's cr_ppm/tier outputs against the shared
// integration vectors (the `tier` op vectors that Rust + TS already agree on),
// making this a 3-way Rust = TS = circuit check. Requires `build/` (run a circom
// --wasm compile first; see check.sh).
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { createRequire } from "node:module";

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const wcBuilder = require(join(here, "build/m4a_cr_tier_js/witness_calculator.js"));
const wasm = readFileSync(join(here, "build/m4a_cr_tier_js/m4a_cr_tier.wasm"));
const wc = await wcBuilder(wasm);

const doc = JSON.parse(readFileSync(join(here, "../integration/vectors/vectors.json"), "utf8"));
const tierVectors = doc.vectors.filter((v) => v.op === "tier");

let pass = 0;
const failures = [];
for (const v of tierVectors) {
  const reserve = BigInt(v.inputs.reserve_sats);
  const supply = BigInt(v.inputs.supply_atoms);
  const price = BigInt(v.inputs.price_e8);
  if (supply === 0n) continue; // circuit covers the CR-defined (supply>0) case

  const num = reserve * price * 1_000_000n;
  const den = supply * 100_000_000_000_000n;
  const cr_q = num / den;
  const cr_r = num % den;

  // Witness layout: [1, cr_ppm, tier, state_commit, ...private].
  const w = await wc.calculateWitness(
    {
      reserve_sats: reserve.toString(),
      supply_atoms: supply.toString(),
      price_e8: price.toString(),
      cr_q: cr_q.toString(),
      cr_r: cr_r.toString(),
    },
    true,
  );
  const circuitCr = w[1].toString();
  const circuitTier = w[2].toString();
  const wantCr = v.cr_ppm;
  const wantTier = String(v.tier);

  if (circuitCr === wantCr && circuitTier === wantTier) {
    pass++;
  } else {
    failures.push(
      `${v.name}: circuit (cr=${circuitCr}, tier=${circuitTier}) ` +
        `!= vector (cr=${wantCr}, tier=${wantTier})`,
    );
  }
}

if (failures.length) {
  console.error("FAIL:\n" + failures.join("\n"));
  process.exit(1);
}
console.log(`circuit ↔ vectors: ${pass} tier vectors match (cr_ppm + tier)`);
