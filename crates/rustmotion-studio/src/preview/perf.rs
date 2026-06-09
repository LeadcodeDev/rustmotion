use std::time::Duration;

/// Median of a set of durations (zero for an empty slice).
pub fn median(mut samples: Vec<Duration>) -> Duration {
    if samples.is_empty() {
        return Duration::ZERO;
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_of_odd_set() {
        let s = vec![
            Duration::from_millis(10),
            Duration::from_millis(30),
            Duration::from_millis(20),
        ];
        assert_eq!(median(s), Duration::from_millis(20));
    }

    #[test]
    fn median_of_empty_is_zero() {
        assert_eq!(median(vec![]), Duration::ZERO);
    }
}
