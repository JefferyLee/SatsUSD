.PHONY: check fmt clippy test vectors verify devnet-up devnet-down recapture-vectors

# Full local gate: formatting, lints, Rust tests, regenerate vectors, cross-language match.
check: fmt clippy test vectors verify

fmt:
	cargo fmt --all -- --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

# Regenerate the canonical test-vector files from the Rust reference.
vectors:
	cargo run -p satusd-vectors --bin satusd-vectors
	cargo run -p satusd-vectors --bin rail-vectors

# Install the TypeScript reference dependencies (Poseidon / secp256k1).
ts-deps:
	cd ts && npm ci

# Verify the TypeScript reference byte-matches the generated vectors.
# Requires `make ts-deps` once (or npm install in ts/).
verify:
	node ts/src/verify.ts
	node ts/src/verify-rail.ts

# Regtest devnet (native binaries): bitcoind + lnd + tapd. See devnet/README.md.
devnet-up:
	./devnet/devnet-up.sh

devnet-down:
	./devnet/devnet-down.sh

# Recapture the real tapd lineage fixtures from a LIVE devnet (bitcoind+lnd+tapd up,
# with a grouped SatUSD asset minted — see integration/lineage_vectors/PROVENANCE.md).
# Regenerates burn_to_sink.hex + lock_anchor.hex and prints the new family_id / sink
# key. The genesis/transfer/grouped_transfer/burn_transfer fixtures are recaptured
# manually (PROVENANCE.md §"manual fixtures").
recapture-vectors:
	@echo "== recapture: burn-to-sink (§5.D16) =="
	cargo run -p satusd-tapd-client --bin capture_burn_vector
	@echo "== recapture: lock-anchor (§5.D3 / §5.D15) =="
	cargo run -p satusd-tapd-client --bin capture_lock_vector
	@echo ""
	@echo "== NEXT (if the devnet asset changed, update the pinned constants) =="
	@echo "  1. FAMILY const  -> crates/satusd-state/src/claim.rs + node.rs  (= family_id printed above)"
	@echo "  2. sink key      -> crates/satusd-ta-proof/src/lib.rs::burn_to_sink_lineage_verifies (= sink_key above)"
	@echo "  3. make vectors && make test   (any remaining drift fails loudly)"
	@echo "  Full procedure: integration/lineage_vectors/PROVENANCE.md"
