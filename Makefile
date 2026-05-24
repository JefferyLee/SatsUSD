.PHONY: check fmt clippy test vectors verify

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

# Verify the TypeScript reference byte-matches the generated vectors.
verify:
	node ts/src/verify.ts
