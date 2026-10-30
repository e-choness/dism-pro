//! Process exit codes (TD-0002 §5.1): a public contract, never renumbered.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Ok = 0,
    Failed = 1,
    Usage = 2,
    /// A plan was printed but not applied: `--yes` (or `--allow-irreversible`) missing.
    ConfirmationRequired = 3,
    /// Some changes failed; the rest were applied.
    Partial = 4,
    Cancelled = 5,
    NeedsElevation = 10,
    PlanStale = 11,
    TargetBusy = 12,
}

impl ExitCode {
    #[must_use]
    pub fn code(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable() {
        let all = [
            (ExitCode::Ok, 0),
            (ExitCode::Failed, 1),
            (ExitCode::Usage, 2),
            (ExitCode::ConfirmationRequired, 3),
            (ExitCode::Partial, 4),
            (ExitCode::Cancelled, 5),
            (ExitCode::NeedsElevation, 10),
            (ExitCode::PlanStale, 11),
            (ExitCode::TargetBusy, 12),
        ];
        for (c, n) in all {
            assert_eq!(c.code(), n);
        }
    }
}
