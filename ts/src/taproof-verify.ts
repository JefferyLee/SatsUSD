// Pins the taproof.ts foundational layer against the shared Rust-generated
// proof.File vectors: every file must parse with a valid sha256 hash chain, and
// every contained proof must re-encode byte-for-byte (lossless TLV round-trip).
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import {
  parseProofFile, parseProof, reencodeProof,
  parseAssetLeaf, assetId, verifyAnchor, verifyLineage, proofGet, tlv,
} from "./taproof.ts";
import { leaf as mssmtLeaf } from "./mssmt.ts";

const here = dirname(fileURLToPath(import.meta.url));
const dir = join(here, "../../integration/lineage_vectors");

function hexToBytes(h: string): Uint8Array {
  const s = h.trim();
  const out = new Uint8Array(s.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(s.substr(i * 2, 2), 16);
  return out;
}
function bytesToHexStr(b: Uint8Array): string {
  let s = "";
  for (const x of b) s += x.toString(16).padStart(2, "0");
  return s;
}
function eq(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

// MS-SMT empty-leaf hash must match tapd's EmptyLeafNode (sha256 of 8 zero bytes).
const emptyLeaf = bytesToHexStr(mssmtLeaf(new Uint8Array(0), 0n).hash);
const EXPECTED_EMPTY = "af5570f5a1810b7af78caf4bc70a660f0df51e42baf91d4de5b2328de0e83dfc";
if (emptyLeaf !== EXPECTED_EMPTY) {
  console.log(`  ✗ mssmt empty-leaf hash ${emptyLeaf} != tapd ${EXPECTED_EMPTY}`);
  process.exit(1);
}
console.log(`  ✓ mssmt empty-leaf hash matches tapd`);

let files = 0, proofs = 0, fail = 0;
for (const name of readdirSync(dir).filter((f) => f.endsWith(".hex")).sort()) {
  const data = hexToBytes(readFileSync(join(dir, name), "utf8"));
  try {
    const pf = parseProofFile(data); // throws on bad magic / hash-chain mismatch / trailing
    let idx = 0;
    let anchored = 0;
    let lastId = "";
    for (const blob of pf.proofs) {
      const p = parseProof(blob);
      if (!eq(reencodeProof(p), blob)) {
        console.log(`  ✗ ${name}: proof ${idx} reencode != original`);
        fail++;
      }
      // asset leaf + asset_id (when present)
      if (proofGet(p, tlv.ASSET_LEAF)) {
        const leaf = parseAssetLeaf(proofGet(p, tlv.ASSET_LEAF)!);
        lastId = bytesToHexStr(assetId(leaf.genesis)).slice(0, 12);
      }
      // on-chain anchoring: header merkle root reconstructs from the tx + merkle proof
      if (proofGet(p, tlv.BLOCK_HEADER)) {
        const a = verifyAnchor(p); // throws AnchorMerkleMismatch on a bad txid/fold/header
        if (a.taprootOutputKey.length === 32) anchored++;
      }
      idx++;
    }
    // Full DL-23 + lineage: every step's asset committed in its anchor output,
    // and each step's input is the previous step's anchored output.
    const head = verifyLineage(pf.proofs.map(parseProof));
    const headKey = bytesToHexStr(head.taprootOutputKey).slice(0, 12);
    proofs += pf.proofs.length;
    files++;
    console.log(`  ✓ ${name}: ${pf.proofs.length} proofs · hash chain · round-trip · anchored ${anchored}/${pf.proofs.length} · LINEAGE OK · asset ${lastId}… · head ${headKey}…`);
  } catch (e: any) {
    console.log(`  ✗ ${name}: ${e?.kind ?? e?.message ?? e}`);
    fail++;
  }
}
console.log(`\ntaproof foundational layer: ${files} files, ${proofs} proofs, fail=${fail}`);
if (fail) process.exit(1);
console.log("OK — proof.File parse + hash chain + TLV round-trip byte-match the Rust vectors.");
