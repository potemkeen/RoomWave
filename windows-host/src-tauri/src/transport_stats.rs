use serde::Serialize;

pub struct Histogram {
    buckets: [u64; 256],
    count: u64,
    max_ms: f64,
}
impl Default for Histogram {
    fn default() -> Self {
        Self {
            buckets: [0; 256],
            count: 0,
            max_ms: 0.0,
        }
    }
}
impl Histogram {
    pub fn add_ns(&mut self, ns: u64) {
        let ms = ns as f64 / 1e6;
        self.buckets[(ms.ceil() as usize).min(255)] += 1;
        self.count += 1;
        self.max_ms = self.max_ms.max(ms);
    }
    pub fn snapshot(&self) -> Summary {
        let quantile = |q: f64| {
            let rank = (self.count as f64 * q).ceil() as u64;
            let mut sum = 0;
            for (i, n) in self.buckets.iter().enumerate() {
                sum += n;
                if sum >= rank {
                    return i as f64;
                }
            }
            255.0
        };
        Summary {
            samples: self.count,
            p50_ms: quantile(0.5),
            p95_ms: quantile(0.95),
            p99_ms: quantile(0.99),
            max_ms: self.max_ms,
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    samples: u64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    max_ms: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rare_delay_is_not_hidden_by_average() {
        let mut h = Histogram::default();
        for _ in 0..98 {
            h.add_ns(1_000_000);
        }
        h.add_ns(15_000_000);
        h.add_ns(300_000_000);
        let s = h.snapshot();
        assert_eq!(s.p50_ms, 1.);
        assert_eq!(s.p99_ms, 15.);
        assert_eq!(s.max_ms, 300.);
    }
}
