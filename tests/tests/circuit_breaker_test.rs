use scp_pool::{CircuitBreaker, CircuitState};

/// Test that circuit breaker opens after reaching failure threshold
#[test]
fn test_circuit_breaker_opens_after_failures() {
    let cb = CircuitBreaker::new(3, 30);

    // Initially closed
    assert!(!cb.is_open());

    // Record failures
    cb.call_failed();
    assert!(!cb.is_open()); // 1 failure

    cb.call_failed();
    assert!(!cb.is_open()); // 2 failures

    cb.call_failed();
    assert!(cb.is_open()); // 3 failures = threshold, now open
}

/// Test that circuit breaker resets after success
#[test]
fn test_circuit_breaker_resets_after_success() {
    let cb = CircuitBreaker::new(3, 30);

    // Open the circuit
    cb.call_failed();
    cb.call_failed();
    cb.call_failed();
    assert!(cb.is_open());

    // Record success
    cb.call_succeeded();
    assert!(!cb.is_open());
    assert_eq!(cb.get_state(), CircuitState::Closed);
}

/// Test that circuit breaker transitions to half-open state after timeout
#[test]
fn test_circuit_breaker_half_open_state() {
    let cb = CircuitBreaker::new(3, 0); // 0 second timeout for immediate transition

    // Open the circuit
    cb.call_failed();
    cb.call_failed();
    cb.call_failed();

    // Verify it's in Open state
    match cb.get_state() {
        CircuitState::Open { .. } => {} // Expected
        _ => panic!("Expected Open state after failures"),
    }

    // With 0s timeout, next is_open() call should transition to HalfOpen
    let _ = cb.is_open(); // This triggers the transition
    assert_eq!(cb.get_state(), CircuitState::HalfOpen);
}

/// Test that circuit breaker can be created and used with custom threshold
#[test]
fn test_circuit_breaker_custom_threshold() {
    let cb = CircuitBreaker::new(5, 30);

    // Verify circuit breaker is initially closed
    assert!(!cb.is_open());

    // Record failures
    cb.call_failed();
    cb.call_failed();
    cb.call_failed();
    cb.call_failed();
    assert!(!cb.is_open()); // 4 failures, not yet open

    cb.call_failed();

    // Circuit should be open (threshold is 5)
    assert!(cb.is_open());
}

/// Test that circuit breaker half-open failure reopens the circuit
#[test]
fn test_circuit_breaker_half_open_failure_reopens() {
    let cb = CircuitBreaker::new(3, 0); // 0 second timeout

    // Open the circuit
    cb.call_failed();
    cb.call_failed();
    cb.call_failed();

    // Transition to HalfOpen
    let _ = cb.is_open(); // This triggers the transition
    assert_eq!(cb.get_state(), CircuitState::HalfOpen);

    // Probe fails
    cb.call_failed();

    // Should be back to Open
    match cb.get_state() {
        CircuitState::Open { .. } => {} // Expected
        _ => panic!("Expected Open state after probe failure"),
    }
}

/// Test that circuit breaker half-open success closes the circuit
#[test]
fn test_circuit_breaker_half_open_success_closes() {
    let cb = CircuitBreaker::new(3, 0); // 0 second timeout

    // Open the circuit
    cb.call_failed();
    cb.call_failed();
    cb.call_failed();

    // Transition to HalfOpen
    let _ = cb.is_open(); // This triggers the transition
    assert_eq!(cb.get_state(), CircuitState::HalfOpen);

    // Probe succeeds
    cb.call_succeeded();

    // Should be back to Closed
    assert!(!cb.is_open());
    assert_eq!(cb.get_state(), CircuitState::Closed);
}
