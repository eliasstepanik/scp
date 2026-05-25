use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Circuit breaker state.
#[derive(Debug, Clone, PartialEq)]
pub enum CircuitState {
    /// Circuit is closed (normal operation).
    Closed,
    /// Circuit is open (rejecting calls).
    Open {
        /// Time when the circuit was opened.
        opened_at: Instant,
    },
    /// Circuit is half-open (allowing probe calls).
    HalfOpen,
}

// TODO(circuit-breaker): Wire this into the PoolManager call path.
// Currently this struct is fully implemented but has zero callers.
// See manager.rs for the integration point — check is_open() before
// dispatching to SharedPool::call(), and call call_failed()/call_succeeded()
// based on the result.

/// Circuit breaker for fault tolerance.
///
/// Implements the circuit breaker pattern to prevent cascading failures
/// by rejecting calls when a service is failing.
pub struct CircuitBreaker {
    state: Mutex<CircuitState>,
    consecutive_failures: AtomicU32,
    failure_threshold: u32,
    probe_timeout: Duration,
}

impl CircuitBreaker {
    /// Creates a new circuit breaker with the given failure threshold and probe timeout.
    pub fn new(failure_threshold: u32, probe_timeout_secs: u64) -> Self {
        Self {
            state: Mutex::new(CircuitState::Closed),
            consecutive_failures: AtomicU32::new(0),
            failure_threshold,
            probe_timeout: Duration::from_secs(probe_timeout_secs),
        }
    }

    /// Returns true if the circuit is open (should reject calls immediately)
    pub fn is_open(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match &*state {
            CircuitState::Closed | CircuitState::HalfOpen => false,
            CircuitState::Open { opened_at } => {
                if opened_at.elapsed() >= self.probe_timeout {
                    // Transition to HalfOpen to allow one probe
                    *state = CircuitState::HalfOpen;
                    false // Let the probe through
                } else {
                    true // Still open, reject
                }
            }
        }
    }

    /// Records a successful call and closes the circuit if it was half-open.
    pub fn call_succeeded(&self) {
        self.consecutive_failures.store(0, Ordering::SeqCst);
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        *state = CircuitState::Closed;
    }

    /// Records a failed call and opens the circuit if the failure threshold is reached.
    pub fn call_failed(&self) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match &*state {
            CircuitState::HalfOpen => {
                // Probe failed, reopen
                *state = CircuitState::Open {
                    opened_at: Instant::now(),
                };
                self.consecutive_failures.store(0, Ordering::SeqCst);
            }
            CircuitState::Closed if failures >= self.failure_threshold => {
                *state = CircuitState::Open {
                    opened_at: Instant::now(),
                };
            }
            _ => {}
        }
    }

    /// Returns the current state of the circuit breaker.
    pub fn get_state(&self) -> CircuitState {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_opens_after_threshold() {
        let cb = CircuitBreaker::new(3, 30);
        assert!(!cb.is_open());
        cb.call_failed();
        cb.call_failed();
        assert!(!cb.is_open()); // 2 failures, not yet open
        cb.call_failed();
        assert!(cb.is_open()); // 3 failures = threshold, now open
    }

    #[test]
    fn test_circuit_breaker_closes_on_success() {
        let cb = CircuitBreaker::new(3, 30);
        cb.call_failed();
        cb.call_failed();
        cb.call_failed();
        assert!(cb.is_open());
        cb.call_succeeded();
        assert!(!cb.is_open());
        assert_eq!(cb.get_state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_half_open_after_timeout() {
        let cb = CircuitBreaker::new(3, 0); // 0 second timeout for test
        cb.call_failed();
        cb.call_failed();
        cb.call_failed();
        // With 0s timeout, is_open() should transition to HalfOpen
        assert!(!cb.is_open()); // Now HalfOpen (probe allowed)
        assert_eq!(cb.get_state(), CircuitState::HalfOpen);
    }

    #[test]
    fn test_circuit_breaker_half_open_failure_reopens() {
        let cb = CircuitBreaker::new(3, 0); // 0 second timeout
        cb.call_failed();
        cb.call_failed();
        cb.call_failed();
        // Transition to HalfOpen
        assert!(!cb.is_open()); // Now HalfOpen (probe allowed)
        assert_eq!(cb.get_state(), CircuitState::HalfOpen);
        // Probe fails
        cb.call_failed();
        // Should be back to Open
        match cb.get_state() {
            CircuitState::Open { .. } => {} // Expected
            _ => panic!("Expected Open state after probe failure"),
        }
    }
}
