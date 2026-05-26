# tap-proof-oracle

A tiny Go program that imports **tapd v0.7.2's own** `asset` + `commitment`
packages to compute ground-truth commitment values, used to reverse-engineer and
validate the Rust `satusd-ta-proof` verifier byte-for-byte.

It is the oracle behind the `project_tapd_proof_format` findings: it cracked the
asset→output-key commitment chain (AssetCommitment key/root, V2 TapCommitment
tapscript leaf), the split-commitment witness stripping, the grouped-asset inner
key, and the tapscript-sibling (`TapscriptPreimage`) hashing — each cross-checked
against the Rust port.

## Usage

```
go run . <asset_leaf_hex_file>
```

Prints (for the decoded `asset.Asset`): the AssetCommitment key, `ac.Root()`, the
V2 `TapscriptRoot`, and — for a `TapscriptPreimage` — the encoded type-5 wire
bytes + tap hash + the `TapscriptRoot(sibling)`. Edit `main.go` to dump whatever
intermediate a given investigation needs.

## Notes

- Requires the lnd-forked protobuf (`go.mod` carries the
  `replace google.golang.org/protobuf => github.com/lightninglabs/protobuf-go-hex-display`
  directive); decode assets via `asset.Decode` and avoid importing the `proof`
  package (it pulls `taprpc`/`lnrpc`).
- Build/run needs network access to fetch the tapd module tree (or a warm Go
  module cache). This is a dev/reference tool, **not** part of `make check`.
