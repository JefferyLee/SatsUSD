pragma circom 2.1.9;

include "smt_fold.circom";

// M4b: SMT membership / non-membership fold to a public root (ADR-0015). The
// gadget lives in smt_fold.circom so the lock-state circuits can reuse it.
component main = SmtFold(256);
