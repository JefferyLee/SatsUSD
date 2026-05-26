//! SatUSD operator backend (MVP, PRD §14.6 M3). The operator-side pieces of the
//! fast redemption: a [`quote`] engine (pricing/fee math that reconciles with the
//! reserve reimbursement the state node computes) and a BTC [`htlc`] builder (the
//! operator's payment leg, atomic with the SatUSD lock via a shared payment hash).
//!
//! The remaining operator flow — watch the chain for the user's HTLC claim,
//! build a `BtcPayoutConfirmation` once it is buried (the SPV evidence the state
//! node re-verifies in `satusd-state::spv`), batch confirmed redemptions into a
//! `ReserveClaim`, and POST it to the state node `/v1/transition/submit` — wires
//! these primitives to the already-tested verifier/node layer.

pub mod htlc;
pub mod observer;
pub mod quote;
pub mod submit;

pub use htlc::{build_btc_htlc, BtcHtlc};
pub use observer::build_payout_confirmation;
pub use quote::{quote, Quote, QuoteError};
pub use submit::{submit_transition, SubmitError};
