#!/usr/bin/env node
// SatUSD live oracle signer daemon (M6 pilot, PRD §5.D7).
//
// Signs BabyJubjub EdDSA over `oracle_message_hash(set_epoch, price_epoch,
// ts_ms, price_e8)` for a 3-of-5 quorum, via circomlibjs (Rust babyjubjub-rs
// `sign` panics on arm64 — M2 #29 note — but its `verify` works, and the iden3
// packed format is identical, so JS signs / Rust verifies, byte-for-byte).
//
//   PORT=8090 KEY_SEED_PREFIX=satusd-oracle node index.mjs
//
//   curl http://127.0.0.1:8090/v1/oracle/pubkeys
//   curl 'http://127.0.0.1:8090/v1/oracle/quorum?price_e8=5000000000000\
//         &set_epoch=7&price_epoch=100&ts_ms=1700000000000'
//
// Dev daemon: hosts all 5 keys derived from KEY_SEED_PREFIX. A real pilot
// deploys 5 INDEPENDENT operators, each holding ONE key (HSM / hardware wallet)
// and running their own instance; the submitter picks 3+ of the 5 signatures.

import { createServer } from "node:http";
import { createHash } from "node:crypto";
import { buildPoseidon, buildEddsa, buildBabyjub } from "circomlibjs";

const PORT = parseInt(process.env.PORT || "8090", 10);
const KEY_SEED_PREFIX = process.env.KEY_SEED_PREFIX || "satusd-oracle";
const SET_SIZE = 5;
const PAIR = "BTC/USD\0"; // 8 bytes, [u8;8] in the OracleMessage struct

const poseidon = await buildPoseidon();
const eddsa = await buildEddsa();
const babyjub = await buildBabyjub();

function keySeed(i) {
  return createHash("sha256").update(`${KEY_SEED_PREFIX}-${i}`).digest();
}

const SECRETS = Array.from({ length: SET_SIZE }, (_, i) => keySeed(i + 1));
const PUBKEYS = SECRETS.map((s) => babyjub.packPoint(eddsa.prv2pub(s)));

const pubkeyHex = (i) => Buffer.from(PUBKEYS[i]).toString("hex");

/// Mirror `satusd_crypto::poseidon::oracle_message_hash`: Poseidon over the 4
/// consensus-relevant inputs, as a field element (signPoseidon takes the field
/// element directly; Rust verify decodes the 32-byte BE form to the same Fr).
function oracleMessageFE(set_epoch, price_epoch, ts_ms, price_e8) {
  return poseidon([
    BigInt(set_epoch),
    BigInt(price_epoch),
    BigInt(ts_ms),
    BigInt(price_e8),
  ]);
}

function signMessage(i, set_epoch, price_epoch, ts_ms, price_e8) {
  const msgFE = oracleMessageFE(set_epoch, price_epoch, ts_ms, price_e8);
  const sig = eddsa.signPoseidon(SECRETS[i], msgFE);
  const sigPacked = eddsa.packSignature(sig); // 64 bytes (pack(R8) || S_LE)
  return {
    domain: "00".repeat(32),
    oracle_id: Buffer.alloc(32, i).toString("hex"),
    oracle_set_epoch: String(set_epoch),
    price_epoch: String(price_epoch),
    timestamp_ms: String(ts_ms),
    pair: Buffer.from(PAIR, "ascii").toString("hex"),
    price_e8: String(price_e8),
    source_commitment: "00".repeat(32),
    signer_pubkey: pubkeyHex(i),
    signature: Buffer.from(sigPacked).toString("hex"),
  };
}

console.log(`[oracle-signer] set size ${SET_SIZE}, key prefix "${KEY_SEED_PREFIX}"`);
for (let i = 0; i < SET_SIZE; i++) {
  console.log(`  [${i}] pubkey ${pubkeyHex(i)}`);
}

const server = createServer((req, res) => {
  const url = new URL(req.url, `http://localhost:${PORT}`);
  try {
    if (url.pathname === "/v1/oracle/pubkeys") {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(
        JSON.stringify({
          pubkeys: Array.from({ length: SET_SIZE }, (_, i) => pubkeyHex(i)),
        }),
      );
      return;
    }
    if (url.pathname === "/v1/oracle/quorum") {
      const price_e8 = url.searchParams.get("price_e8");
      if (!price_e8) {
        res.writeHead(400);
        res.end("price_e8 required");
        return;
      }
      const set_epoch = url.searchParams.get("set_epoch") || "7";
      const price_epoch =
        url.searchParams.get("price_epoch") || String(Math.floor(Date.now() / 1000));
      const ts_ms = url.searchParams.get("ts_ms") || String(Date.now());
      const messages = Array.from({ length: SET_SIZE }, (_, i) =>
        signMessage(i, set_epoch, price_epoch, ts_ms, price_e8),
      );
      res.writeHead(200, { "content-type": "application/json" });
      res.end(
        JSON.stringify({
          set_epoch,
          price_epoch,
          timestamp_ms: ts_ms,
          price_e8,
          messages,
        }),
      );
      return;
    }
    res.writeHead(404);
    res.end("not found");
  } catch (e) {
    console.error("[oracle-signer] error:", e);
    res.writeHead(500);
    res.end(String(e.message || e));
  }
});

server.listen(PORT, () => {
  console.log(`[oracle-signer] listening on http://localhost:${PORT}`);
  console.log("  GET /v1/oracle/pubkeys");
  console.log("  GET /v1/oracle/quorum?price_e8=<N>[&set_epoch=N][&price_epoch=N][&ts_ms=N]");
});
