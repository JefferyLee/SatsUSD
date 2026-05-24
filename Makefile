.PHONY: check fmt clippy test vectors verify devnet-up devnet-down

# Full local gate: formatting, lints, Rust tests, regenerate vectors, cross-language match.
check: fmt clippy test vectors verify

fmt:
	cargo fmt --all -- --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

# Regenerate the canonical test-vector file from the Rust reference.
vectors:
	cargo run -p satusd-vectors

# Install the TypeScript reference dependencies (Poseidon / secp256k1).
ts-deps:
	cd ts && npm ci

# Verify the TypeScript reference byte-matches the generated vectors.
# Requires `make ts-deps` once (or npm install in ts/).
verify:
	node ts/src/verify.ts

# Regtest devnet (native binaries): bitcoind + lnd + tapd. See devnet/README.md.
devnet-up:
	./devnet/devnet-up.sh

devnet-down:
	./devnet/devnet-down.sh
