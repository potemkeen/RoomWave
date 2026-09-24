use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

/// Same monotonic timebase as WASAPI's QPC timestamps, expressed in nanoseconds.
pub struct Clock(i64);
impl Clock {
    pub fn new() -> Self {
        let mut frequency = 0;
        // Windows guarantees QPC on supported Windows versions.
        unsafe { QueryPerformanceFrequency(&mut frequency) };
        assert!(frequency > 0);
        Self(frequency)
    }
    pub fn now(&self) -> i64 {
        let mut counter = 0;
        unsafe { QueryPerformanceCounter(&mut counter) };
        (i128::from(counter) * 1_000_000_000 / i128::from(self.0)) as i64
    }
}
