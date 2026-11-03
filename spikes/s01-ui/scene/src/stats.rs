//! Frame-time and timing summaries.

use std::fmt;
use std::time::Duration;

/// Nearest-rank percentiles of a sample, in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Summary {
    pub n: usize,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
    pub mean: f64,
}

impl Summary {
    #[must_use]
    pub fn of(samples: &[Duration]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        let mut ms: Vec<f64> = samples.iter().map(|d| d.as_secs_f64() * 1e3).collect();
        ms.sort_by(f64::total_cmp);
        let rank = |p: f64| ms[((p * ms.len() as f64).ceil() as usize).clamp(1, ms.len()) - 1];
        Some(Self {
            n: ms.len(),
            p50: rank(0.50),
            p95: rank(0.95),
            p99: rank(0.99),
            max: ms[ms.len() - 1],
            mean: ms.iter().sum::<f64>() / ms.len() as f64,
        })
    }
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "n {} p50 {:.2} p95 {:.2} p99 {:.2} max {:.2} mean {:.2} ms",
            self.n, self.p50, self.p95, self.p99, self.max, self.mean
        )
    }
}

/// The median of a sample (lower middle for an even count).
#[must_use]
pub fn median(samples: &mut [f64]) -> Option<f64> {
    samples.sort_by(f64::total_cmp);
    samples.get((samples.len().max(1) - 1) / 2).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank() {
        let d: Vec<Duration> = (1..=100).map(Duration::from_millis).collect();
        let s = Summary::of(&d).map(|s| (s.p50, s.p99, s.max));
        assert_eq!(s, Some((50.0, 99.0, 100.0)));
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(Summary::of(&[]), None);
    }
}
