use crate::retry::exponential_duration;
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
    pub max_redirects: u32,
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
            // Tracker requests carry fleet/admin credentials and security decisions.
            // Never forward them across redirects or accept a redirected membership/ticket body.
            max_redirects: 0,
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
            // HTTP sync는 command payload를 운반하므로 redirect hop에서 transport
            // security 검증이 우회되지 않게 base URL 자체만 요청한다.
            max_redirects: 0,
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
        let connect = exponential_duration(
            policy.connect_base,
            attempt as u32,
            Some(policy.connect_cap),
        );
        let read = exponential_duration(policy.read_base, attempt as u32, Some(policy.read_cap));

        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(connect))
            .timeout_recv_response(Some(read))
            .timeout_recv_body(Some(read))
            .max_redirects(policy.max_redirects)
            .build()
            .into();

        match f(&agent) {
            Ok(v) => return Ok(v),
            Err(err) => {
                let retryable = is_retryable_error(&err);
                last_err = Some(anyhow::anyhow!(err));

                if !retryable || attempt + 1 >= attempts {
                    return Err(last_err.expect("last_err must be set"));
                }

                let backoff = exponential_duration(policy.backoff_base, attempt as u32, None);
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
        ureq::Error::StatusCode(code) => {
            let code = *code;
            code == 408 || code == 429 || (500..=599).contains(&code)
        }
        ureq::Error::Timeout(_)
        | ureq::Error::Io(_)
        | ureq::Error::HostNotFound
        | ureq::Error::ConnectionFailed => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_requests_never_follow_redirects() {
        assert_eq!(RetryPolicy::tracker().max_redirects, 0);
    }
}
