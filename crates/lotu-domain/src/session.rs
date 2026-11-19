//! Session lifecycle state machine.
//!
//! M1 only needs the video session; the `Recording` state arrives with M3 and is
//! deliberately absent so the compiler rejects it until the contract changes.

use std::fmt;

use crate::error::DomainError;

/// Lifecycle of the single live session owned by the application.
///
/// Valid transitions (M1):
///
/// ```text
/// Idle -> Starting -> Streaming -> Stopping -> Idle
///             |            |
///             v            v
///           Error <--------+
///             |
///             v
///            Idle
/// ```
///
/// `Starting -> Error` and `Streaming -> Error` cover failures while opening the
/// stream and while it is running. `Error -> Idle` is the reset applied by the
/// next `start_session` (or by an explicit stop of a failed session).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionState {
    Idle,
    Starting,
    Streaming,
    Stopping,
    Error,
}

impl SessionState {
    /// Every state of the M1 machine, handy for exhaustive tests.
    pub const ALL: [SessionState; 5] = [
        SessionState::Idle,
        SessionState::Starting,
        SessionState::Streaming,
        SessionState::Stopping,
        SessionState::Error,
    ];

    /// Returns `true` when `next` is a valid successor of the current state.
    ///
    /// Self-transitions are not valid: the actor must not re-enter a state it is
    /// already in.
    pub fn can_transition_to(self, next: SessionState) -> bool {
        matches!(
            (self, next),
            (SessionState::Idle, SessionState::Starting)
                | (SessionState::Starting, SessionState::Streaming)
                | (SessionState::Starting, SessionState::Error)
                | (SessionState::Streaming, SessionState::Stopping)
                | (SessionState::Streaming, SessionState::Error)
                | (SessionState::Stopping, SessionState::Idle)
                | (SessionState::Error, SessionState::Idle)
        )
    }

    /// Applies one transition, rejecting the ones the contract does not allow.
    pub fn transition(self, next: SessionState) -> Result<SessionState, DomainError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(DomainError::InvalidSessionTransition(self, next))
        }
    }
}

impl fmt::Display for SessionState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            SessionState::Idle => "idle",
            SessionState::Starting => "starting",
            SessionState::Streaming => "streaming",
            SessionState::Stopping => "stopping",
            SessionState::Error => "error",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Transitions listed in `docs/m1-contract.md` 4.2.
    const VALID: [(SessionState, SessionState); 7] = [
        (SessionState::Idle, SessionState::Starting),
        (SessionState::Starting, SessionState::Streaming),
        (SessionState::Starting, SessionState::Error),
        (SessionState::Streaming, SessionState::Stopping),
        (SessionState::Streaming, SessionState::Error),
        (SessionState::Stopping, SessionState::Idle),
        (SessionState::Error, SessionState::Idle),
    ];

    fn is_valid(from: SessionState, to: SessionState) -> bool {
        VALID.contains(&(from, to))
    }

    #[test]
    fn accepts_the_contract_transitions() {
        for (from, to) in VALID {
            assert!(from.can_transition_to(to), "{from} -> {to} must be valid");
            assert_eq!(from.transition(to), Ok(to), "{from} -> {to} must apply");
        }
    }

    #[test]
    fn rejects_every_other_transition() {
        for from in SessionState::ALL {
            for to in SessionState::ALL {
                if is_valid(from, to) {
                    continue;
                }
                assert!(!from.can_transition_to(to), "{from} -> {to} is invalid");
                assert_eq!(
                    from.transition(to),
                    Err(DomainError::InvalidSessionTransition(from, to)),
                    "{from} -> {to} is invalid"
                );
            }
        }
    }

    #[test]
    fn rejects_self_transitions() {
        for state in SessionState::ALL {
            assert!(!state.can_transition_to(state), "{state} -> {state}");
        }
    }

    #[test]
    fn walks_the_happy_path_back_to_idle() {
        let state = SessionState::Idle
            .transition(SessionState::Starting)
            .expect("idle -> starting")
            .transition(SessionState::Streaming)
            .expect("starting -> streaming")
            .transition(SessionState::Stopping)
            .expect("streaming -> stopping")
            .transition(SessionState::Idle)
            .expect("stopping -> idle");
        assert_eq!(state, SessionState::Idle);
    }

    #[test]
    fn walks_the_failure_path_back_to_idle() {
        let state = SessionState::Idle
            .transition(SessionState::Starting)
            .expect("idle -> starting")
            .transition(SessionState::Error)
            .expect("starting -> error")
            .transition(SessionState::Idle)
            .expect("error -> idle");
        assert_eq!(state, SessionState::Idle);
    }

    #[test]
    fn streaming_can_fail_but_not_restart_directly() {
        let streaming = SessionState::Streaming;
        assert!(streaming.transition(SessionState::Error).is_ok());
        assert!(streaming.transition(SessionState::Starting).is_err());
    }

    #[test]
    fn displays_lowercase_names() {
        assert_eq!(SessionState::Idle.to_string(), "idle");
        assert_eq!(SessionState::Starting.to_string(), "starting");
        assert_eq!(SessionState::Streaming.to_string(), "streaming");
        assert_eq!(SessionState::Stopping.to_string(), "stopping");
        assert_eq!(SessionState::Error.to_string(), "error");
    }
}
