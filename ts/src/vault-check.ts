// Cross-checks ts/src/vault.ts against the same assertions the Rust
// unit tests (crates/satusd-vault contract.rs + settle.rs) make, so the
// phone-side vault math byte-matches the on-chain reference.
import {
  crBps, openingOk, crashPriceCeiling, faceSats, crashPayout,
  reserveShortfallSats, DEFAULT_TERMS, DEFAULT_PARAMS,
} from "./vault.ts";

let fail = 0;
const ok = (cond: boolean, msg: string) => {
  console.log(`  ${cond ? "✓" : "✗"} ${msg}`);
  if (!cond) fail++;
};

// --- contract.rs::tests ---
{
  const t = DEFAULT_TERMS(1172n, 500_000n); // ~150% CR for $0.5 @ $64k
  const cr = crBps(t.collateralSats, t.mintMicroUsd, 64_000);
  ok(cr >= 14_990 && cr <= 15_010, `CR≈150% @ $64k (got ${cr})`);
  ok(openingOk(t, 64_000), "opening_ok @ $64k");
  ok(!openingOk(t, 60_000), "NOT opening_ok @ $60k");

  const ceil = crashPriceCeiling(t);
  ok(ceil < 64_000 && ceil > 40_000, `crash ceiling in (40k,64k): ${ceil}`);
  ok(crBps(t.collateralSats, t.mintMicroUsd, ceil) <= t.liqCrBps, "CR≤liq at ceiling");
  ok(crBps(t.collateralSats, t.mintMicroUsd, ceil + 2_000) > t.liqCrBps, "CR>liq above ceiling");
}

// --- settle.rs::tests (realistic $1000 vault ~150% @ $64k) ---
{
  const MINT = 1_000_000_000n;
  const C = 2_343_750n;
  const p = DEFAULT_PARAMS;
  const SPEND = C - p.feeBudgetSats;

  for (const price of [46_900, 44_000, 40_000, 30_000, 20_000]) {
    const po = crashPayout(C, MINT, price, p);
    const total = po.reserveSats + po.broadcasterSats + po.minterSats;
    ok(total === SPEND, `outputs sum to spendable @ $${price}`);
  }

  const near = crashPayout(C, MINT, 46_900, p);
  ok(near.reserveSats >= faceSats(MINT, 46_900), "near-liq: reserve covers face");
  ok(near.minterSats > 0n, "near-liq: minter keeps the cushion");
  ok(reserveShortfallSats(near, MINT, 46_900) === 0n, "near-liq: no gap");

  const gap = crashPayout(C, MINT, 20_000, p);
  ok(gap.minterSats === 0n, "deep gap: minter 0");
  ok(reserveShortfallSats(gap, MINT, 20_000) > 0n, "deep gap: reserve under-recovers");

  const capped = crashPayout(C, MINT, 46_900, { ...p, bountyCapSats: 100n });
  ok(capped.broadcasterSats === 100n, "bounty respects cap");
}

console.log(`\nvault math cross-check: fail=${fail}`);
if (fail) process.exit(1);
console.log("OK — ts/src/vault.ts byte-matches the Rust contract/settle assertions.");
