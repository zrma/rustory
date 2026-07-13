use std::time::Duration;

pub(crate) fn exponential_duration(
    base: Duration,
    attempt: u32,
    cap: Option<Duration>,
) -> Duration {
    let factor = 1u32.checked_shl(attempt).unwrap_or(u32::MAX);
    let got = base
        .checked_mul(factor)
        .unwrap_or_else(|| cap.unwrap_or(Duration::MAX));
    match cap {
        Some(cap) if got > cap => cap,
        _ => got,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_duration_saturates_to_cap_on_multiplication_overflow() {
        let base = Duration::from_secs(u64::MAX / 8);
        let cap = Duration::from_secs(u64::MAX / 4);

        assert_eq!(exponential_duration(base, 4, Some(cap)), cap);
    }

    #[test]
    fn exponential_duration_saturates_to_duration_max_without_cap_on_overflow() {
        let base = Duration::from_secs(u64::MAX / 8);

        assert_eq!(exponential_duration(base, 4, None), Duration::MAX);
    }
}
