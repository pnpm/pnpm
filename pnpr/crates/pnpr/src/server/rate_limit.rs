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
    WebLogin(Option<IpAddr>),
}

struct AttemptRecord {
    count: u32,
    first_attempt: Instant,
}

struct RateLimiterInner {
    map: HashMap<RateLimitKey, AttemptRecord>,
    last_eviction: Instant,
}

/// Tracks failed login attempts to protect against password brute-forcing
/// and CPU exhaustion from repetitive bcrypt calls.
pub(crate) struct LoginRateLimiter {
    window: Duration,
    max_ip_user_attempts: u32,
    max_ip_attempts: u32,
    max_user_attempts: u32,
    records: Mutex<RateLimiterInner>,
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
            records: Mutex::new(RateLimiterInner {
                map: HashMap::new(),
                last_eviction: Instant::now(),
            }),
        }
    }

    fn evict_expired(&self, inner: &mut RateLimiterInner, now: Instant) {
        if now.duration_since(inner.last_eviction) >= Duration::from_secs(30)
            || inner.map.len() > 1024
        {
            inner.map.retain(|_, record| now.duration_since(record.first_attempt) < self.window);
            inner.last_eviction = now;
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
        let mut inner = self.records.lock().unwrap();
        self.evict_expired(&mut inner, now);

        if let Some(ip_address) = client_ip {
            self.check_key(
                &inner.map,
                &RateLimitKey::IpUser(ip_address, username.to_string()),
                self.max_ip_user_attempts,
                now,
            )?;
            self.check_key(&inner.map, &RateLimitKey::Ip(ip_address), self.max_ip_attempts, now)?;
        } else {
            self.check_key(
                &inner.map,
                &RateLimitKey::User(username.to_string()),
                self.max_user_attempts,
                now,
            )?;
        }

        Ok(())
    }

    /// Check and record a web login session creation attempt from `client_ip`.
    /// Returns `Err(retry_after_seconds)` if rate limited.
    pub(crate) fn check_and_record_web_login(&self, client_ip: Option<IpAddr>) -> Result<(), u64> {
        let now = Instant::now();
        let mut inner = self.records.lock().unwrap();
        self.evict_expired(&mut inner, now);

        let key = RateLimitKey::WebLogin(client_ip);
        self.check_key(&inner.map, &key, self.max_ip_attempts, now)?;

        inner.map
            .entry(key)
            .and_modify(|record| {
                if now.duration_since(record.first_attempt) >= self.window {
                    record.count = 1;
                    record.first_attempt = now;
                } else {
                    record.count += 1;
                }
            })
            .or_insert(AttemptRecord { count: 1, first_attempt: now });

        Ok(())
    }

    /// Record a failed login attempt for `client_ip` and `username`.
    pub(crate) fn record_failure(&self, client_ip: Option<IpAddr>, username: &str) {
        let now = Instant::now();
        let mut inner = self.records.lock().unwrap();

        let keys = if let Some(ip_address) = client_ip {
            vec![
                RateLimitKey::IpUser(ip_address, username.to_string()),
                RateLimitKey::Ip(ip_address),
            ]
        } else {
            vec![RateLimitKey::User(username.to_string())]
        };

        for key in keys {
            inner.map
                .entry(key)
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

    /// Clear failure count for `(client_ip, username)` or `username` on successful authentication.
    pub(crate) fn record_success(&self, client_ip: Option<IpAddr>, username: &str) {
        let mut inner = self.records.lock().unwrap();
        if let Some(ip_address) = client_ip {
            inner.map.remove(&RateLimitKey::IpUser(ip_address, username.to_string()));
        } else {
            inner.map.remove(&RateLimitKey::User(username.to_string()));
        }
    }
}
