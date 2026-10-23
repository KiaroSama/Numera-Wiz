//! Credential time is sampled under one lock and advances while the wall clock stalls.
use nw_domain::{DomainError, Result};
use std::{sync::Mutex, time::{Duration, Instant}};

struct Reading {
    wall: u64,
    logical: Duration,
    measured: Instant,
    blocked: bool,
}

impl Reading {
    fn observe(&mut self, wall: u64, elapsed: Duration) -> Result<u64> {
        if self.blocked || wall < self.wall {
            self.blocked = true;
            return Err(DomainError::new("CLOCK_ROLLBACK", "Scoped credential time moved backwards; restart the owner and issue fresh credentials"));
        }
        let advanced = self.logical.checked_add(elapsed).ok_or_else(|| DomainError::new("CLOCK_ROLLBACK", "Credential clock exceeded its supported range"))?;
        self.logical = advanced.max(Duration::from_secs(wall));
        self.wall = wall;
        Ok(self.logical.as_secs())
    }
}

pub(super) struct SessionClock {
    source: Box<dyn Fn() -> u64 + Send + Sync>,
    reading: Mutex<Reading>,
}

impl Default for SessionClock {
    fn default() -> Self {
        Self::with_source(Box::new(nw_platform::epoch_seconds))
    }
}

impl SessionClock {
    fn with_source(source: Box<dyn Fn() -> u64 + Send + Sync>) -> Self {
        Self { source, reading: Mutex::new(Reading { wall: 0, logical: Duration::ZERO, measured: Instant::now(), blocked: false }) }
    }

    pub(super) fn now(&self) -> Result<u64> {
        let mut state = self.reading.lock().map_err(|_| DomainError::new("INTERNAL_ERROR", "Credential clock lock poisoned"))?;
        // Sample inside the lock: an older request's captured time is never a rollback proof.
        let wall = (self.source)();
        let measured = Instant::now();
        let elapsed = measured.duration_since(state.measured);
        let result = state.observe(wall, elapsed);
        state.measured = measured;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stalled_wall_time_cannot_extend_idle_or_absolute_credential_lifetimes() {
        let mut reading = Reading { wall: 20, logical: Duration::from_secs(20), measured: Instant::now(), blocked: false };
        assert_eq!(reading.observe(20, Duration::from_millis(200)).unwrap(), 20);
        assert_eq!(reading.observe(20, Duration::from_millis(900)).unwrap(), 21);
        assert_eq!(reading.observe(20, Duration::from_secs(7200)).unwrap(), 7221);
    }
    #[test]
    fn backward_wall_time_fails_closed_for_the_remaining_owner_epoch() {
        use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
        let wall = Arc::new(AtomicU64::new(20));
        let source = wall.clone();
        let clock = SessionClock::with_source(Box::new(move || source.load(Ordering::SeqCst)));
        assert_eq!(clock.now().unwrap(), 20);
        wall.store(19, Ordering::SeqCst);
        assert_eq!(clock.now().unwrap_err().code, "CLOCK_ROLLBACK");
        wall.store(21, Ordering::SeqCst);
        assert_eq!(clock.now().unwrap_err().code, "CLOCK_ROLLBACK");
    }
}
