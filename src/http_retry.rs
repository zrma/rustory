use anyhow::Result;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub attempts: usize,
    pub connect_base: Duration,
    pub connect_cap: Duration,
    pub read_base: Duration,
    pub read_cap: Duration,
    pub backoff_base: Duration,
}

impl RetryPolicy {
    pub fn tracker() -> Self {
        Self {
            attempts: 3,
            // tracker는 fallback(peer_book)으로 빨리 넘어가야 하므로 base timeout을 짧게 둔다.
            connect_base: Duration::from_millis(300),
            connect_cap: Duration::from_secs(2),
            read_base: Duration::from_secs(1),
            read_cap: Duration::from_secs(5),
            backoff_base: Duration::from_millis(100),
        }
    }

    pub fn transport() -> Self {
        Self {
            attempts: 3,
            connect_base: Duration::from_millis(500),
            connect_cap: Duration::from_secs(5),
            read_base: Duration::from_secs(3),
            read_cap: Duration::from_secs(30),
            backoff_base: Duration::from_millis(200),
        }
    }
}

pub fn request_with_retry<T, F>(policy: RetryPolicy, mut f: F) -> Result<T>
where
    F: FnMut(&ureq::Agent) -> std::result::Result<T, ureq::Error>,
{
    let attempts = policy.attempts.max(1);
    let mut last_err: Option<anyhow::Error> = None;

    for attempt in 0..attempts {
        let connect = exp_duration(
            policy.connect_base,
            attempt as u32,
            Some(policy.connect_cap),
        );
        let read = exp_duration(policy.read_base, attempt as u32, Some(policy.read_cap));

        let agent = ureq::AgentBuilder::new()
            .timeout_connect(connect)
            .timeout_read(read)
            .build();

        match f(&agent) {
            Ok(v) => return Ok(v),
            Err(err) => {
                let retryable = is_retryable_error(&err);
                last_err = Some(anyhow::anyhow!(err));

                if !retryable || attempt + 1 >= attempts {
                    return Err(last_err.expect("last_err must be set"));
                }

                let backoff = exp_duration(policy.backoff_base, attempt as u32, None);
                if backoff > Duration::from_millis(0) {
                    std::thread::sleep(backoff);
                }
            }
        }
    }

    Err(last_err.expect("attempts must be >= 1"))
}

fn is_retryable_error(err: &ureq::Error) -> bool {
    match err {
        ureq::Error::Transport(_) => true,
        ureq::Error::Status(code, _) => {
            let code = *code;
            code == 408 || code == 429 || (500..=599).contains(&code)
        }
    }
}

fn exp_duration(base: Duration, attempt: u32, cap: Option<Duration>) -> Duration {
    let factor = 1u32.checked_shl(attempt).unwrap_or(u32::MAX);
    let got = base.checked_mul(factor).unwrap_or(base);
    match cap {
        Some(cap) if got > cap => cap,
        _ => got,
    }
}
