//! The conversion phase machine (spec 02 §3).
//!
//! DISPUTE is an orthogonal overlay, not a phase: evidence may be
//! submitted at any time and never blocks REFUND, so it does not
//! appear in the transition table.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Quote,
    Lock,
    Settle,
    Refund,
}

/// Whether `from → to` is a legal transition. `collapsed` is the
/// spec 02 §3.5 allowance: a rail MAY collapse LOCK+SETTLE into one
/// atomic step (Rail-0), in which case QUOTE → SETTLE is direct and
/// no refund machinery is reachable.
pub fn may_transition(from: Phase, to: Phase, collapsed: bool) -> bool {
    use Phase::*;
    match (from, to) {
        (Quote, Lock) => !collapsed,
        (Quote, Settle) => collapsed,
        (Lock, Settle) => !collapsed,
        (Lock, Refund) => !collapsed,
        _ => false,
    }
}

/// Terminal phases: a conversion ends in exactly one of these.
pub fn is_terminal(p: Phase) -> bool {
    matches!(p, Phase::Settle | Phase::Refund)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Phase::*;

    const ALL: [Phase; 4] = [Quote, Lock, Settle, Refund];

    #[test]
    fn standard_machine_paths() {
        assert!(may_transition(Quote, Lock, false));
        assert!(may_transition(Lock, Settle, false));
        assert!(may_transition(Lock, Refund, false));
        assert!(!may_transition(Quote, Settle, false));
        assert!(
            !may_transition(Quote, Refund, false),
            "nothing locked, nothing to refund"
        );
    }

    #[test]
    fn collapsed_machine_paths() {
        assert!(may_transition(Quote, Settle, true));
        assert!(!may_transition(Quote, Lock, true));
        // S1 note: refund is unreachable because lock is unreachable.
        for from in ALL {
            assert!(!may_transition(from, Refund, true));
        }
    }

    #[test]
    fn terminals_have_no_exits() {
        for collapsed in [false, true] {
            for to in ALL {
                assert!(!may_transition(Settle, to, collapsed));
                assert!(!may_transition(Refund, to, collapsed));
            }
        }
    }
}
