use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Mutex,
    time::{Duration, Instant},
};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RateLimitKey {
    Ip(IpAddr),
    IpUser(IpAddr, String),
    User(String),
}

struct AttemptRecord {
    count: u32,
    first_attempt: Instant,
}

/// Tracks failed login attempts to protect against password brute-forcing
/// and CPU exhaustion from repetitive bcrypt calls.
pub(crate) struct LoginRateLimiter {
    window: Duration,
    max_ip_user_attempts: u32,
    max_ip_attempts: u32,
    max_user_attempts: u32,
    records: Mutex<HashMap<RateLimitKey, AttemptRecord>>,
}

impl Default for LoginRateLimiter {
    fn default() -> Self {
        Self::new(Duration::from_mins(1), 5, 20, 5)
    }
}

impl LoginRateLimiter {
    pub(crate) fn new(
        window: Duration,
        max_ip_user_attempts: u32,
        max_ip_attempts: u32,
        max_user_attempts: u32,
    ) -> Self {
        Self {
            window,
            max_ip_user_attempts,
            max_ip_attempts,
            max_user_attempts,
            records: Mutex::new(HashMap::new()),
        }
    }

    fn check_key(
        &self,
        map: &HashMap<RateLimitKey, AttemptRecord>,
        key: &RateLimitKey,
        max_attempts: u32,
        now: Instant,
    ) -> Result<(), u64> {
        if let Some(record) = map.get(key)
            && record.count >= max_attempts
        {
            let elapsed = now.duration_since(record.first_attempt);
            if elapsed < self.window {
                let retry_after = self.window
                    .saturating_sub(elapsed)
                    .as_secs()
                    .max(1);
                return Err(retry_after);
            }
        }
        Ok(())
    }

    /// Check if a login attempt from `client_ip` for `username` is currently throttled.
    /// Returns `Err(retry_after_seconds)` if rate limited.
    pub(crate) fn check(&self, client_ip: Option<IpAddr>, username: &str) -> Result<(), u64> {
        let now = Instant::now();
        let mut map = self.records.lock().unwrap();

        // Evict expired entries
        map.retain(|_, record| now.duration_since(record.first_attempt) < self.window);

        self.check_key(
            &map,
            &RateLimitKey::User(username.to_string()),
            self.max_user_attempts,
            now,
        )?;
        if let Some(ip_address) = client_ip {
            self.check_key(
                &map,
                &RateLimitKey::IpUser(ip_address, username.to_string()),
                self.max_ip_user_attempts,
                now,
            )?;
            self.check_key(&map, &RateLimitKey::Ip(ip_address), self.max_ip_attempts, now)?;
        }

        Ok(())
    }

    /// Record a failed login attempt for `client_ip` and `username`.
    pub(crate) fn record_failure(&self, client_ip: Option<IpAddr>, username: &str) {
        let now = Instant::now();
        let mut map = self.records.lock().unwrap();

        let mut keys = vec![RateLimitKey::User(username.to_string())];
        if let Some(ip_address) = client_ip {
            keys.push(RateLimitKey::IpUser(ip_address, username.to_string()));
            keys.push(RateLimitKey::Ip(ip_address));
        }

        for key in keys {
            map.entry(key)
                .and_modify(|record| {
                    if now.duration_since(record.first_attempt) >= self.window {
                        record.count = 1;
                        record.first_attempt = now;
                    } else {
                        record.count += 1;
                    }
                })
                .or_insert(AttemptRecord { count: 1, first_attempt: now });
        }
    }

    /// Clear failure count for `username` and `(client_ip, username)` on successful authentication.
    pub(crate) fn record_success(&self, client_ip: Option<IpAddr>, username: &str) {
        let mut map = self.records.lock().unwrap();
        map.remove(&RateLimitKey::User(username.to_string()));
        if let Some(ip_address) = client_ip {
            map.remove(&RateLimitKey::IpUser(ip_address, username.to_string()));
        }
    }
}
