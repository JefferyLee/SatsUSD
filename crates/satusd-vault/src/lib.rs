//! SatUSD vaults — mint-as-position-opening (spec 06).
//!
//! A vault is a DLC collateral position: the minter locks BTC in a
//! P2TR funding output co-controlled with the reserve, mints
//! face-value SatUSD against it, and the position is settled against
//! the oracle by pre-signed crash-bucket checkpoint CETs.
//!
//! This crate owns the BTC-layer math the construction needs:
//! - [`funding`] — the vault funding output (single-leaf {refund}
//!   tree, internal key = MuSig2(minter, reserve) in the end state),
//!   reusing the rail-1 adaptor/CET primitives for settlement.
//!
//! Differences from a rail-1 redemption (proposal 0001 / satusd-rail1):
//! the counterparty is the reserve (not an LP); the tree carries no
//! TA commitment (collateral is pure BTC, the minted SatUSD is issued
//! separately); and the pre-signed CETs are crash buckets only
//! (checkpoints), not a full settlement spectrum.

pub mod cet;
pub mod contract;
pub mod funding;
pub mod musig;
pub mod settle;
