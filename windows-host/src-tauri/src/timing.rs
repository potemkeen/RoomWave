pub use crate::platform::Clock;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug)]
pub struct ClockSample {
    pub offset_ns: i64, // Android monotonic time minus Windows QPC time.
    pub rtt_ns: i64,
    received_ns: i64,
}

#[derive(Default)]
pub struct ClockSync(VecDeque<ClockSample>);
impl ClockSync {
    pub fn observe(&mut self, t1: i64, t2: i64, t3: i64, t4: i64) {
        // The remote timestamps are untrusted; subtract in a wider type.
        let total = i128::from(t4) - i128::from(t1);
        let processing = i128::from(t3) - i128::from(t2);
        if !(0..=5_000_000_000).contains(&total) || processing < 0 {
            return;
        }
        let rtt = total - processing;
        if !(0..=1_000_000_000).contains(&rtt) {
            return;
        }
        self.0.push_back(ClockSample {
            offset_ns: (((i128::from(t2) - i128::from(t1)) + (i128::from(t3) - i128::from(t4))) / 2)
                as i64,
            rtt_ns: rtt as i64,
            received_ns: t4,
        });
        while self.0.len() > 12 {
            self.0.pop_front();
        }
    }
    /// Prefer the least congested exchange; expire samples to follow clock drift.
    pub fn best(&self, now: i64) -> Option<ClockSample> {
        self.0
            .iter()
            .filter(|s| (0..=10_000_000_000).contains(&(now - s.received_ns)))
            .min_by_key(|s| s.rtt_ns)
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removes_receiver_processing_time_and_supports_negative_offset() {
        let mut sync = ClockSync::default();
        sync.observe(1_000_000_000, 912_000_000, 915_000_000, 1_027_000_000);
        let s = sync.best(1_027_000_000).unwrap();
        assert_eq!(s.offset_ns, -100_000_000);
        assert_eq!(s.rtt_ns, 24_000_000);
    }
    #[test]
    fn rejects_invalid_samples_prefers_low_rtt_and_expires() {
        let mut sync = ClockSync::default();
        sync.observe(0, 100, 101, 20);
        sync.observe(50, 153, 154, 57);
        sync.observe(70, 170, 190, 75);
        sync.observe(70, i64::MIN, i64::MAX, 80);
        assert_eq!(sync.best(80).unwrap().offset_ns, 100);
        assert_eq!(sync.best(80).unwrap().rtt_ns, 6);
        assert!(sync.best(11_000_000_000).is_none());
    }
}
