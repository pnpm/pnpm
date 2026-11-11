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
    WrongWebLoginCode(Option<IpAddr>),
}

/// A pool of at most [`MAX_WEB_LOGIN_RECORDS`] records, for keys an anonymous
/// client can create one of for every address it sends from. Login starts
/// and wrong codes have separate pools, so guesses cannot use up the records
/// that starting a login needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CappedPool {
    WebLogin = 0,
    WrongWebLoginCode = 1,
}

impl RateLimitKey {
    fn capped_pool(&self) -> Option<CappedPool> {
        match self {
            RateLimitKey::WebLogin(_) => Some(CappedPool::WebLogin),
            RateLimitKey::WrongWebLoginCode(_) => Some(CappedPool::WrongWebLoginCode),
            _ => None,
        }
    }
}

const MAX_WEB_LOGIN_RECORDS: usize = 1024;
const EVICTION_MAP_THRESHOLD: usize = 1024;
const EVICTION_INTERVAL: Duration = Duration::from_secs(30);
const FULL_EVICTION_COOLDOWN: Duration = Duration::from_secs(1);

struct AttemptRecord {
    count: u32,
    first_attempt: Instant,
}

struct RateLimiterInner {
    map: HashMap<RateLimitKey, AttemptRecord>,
    /// Live records per [`CappedPool`], indexed by its discriminant.
    capped_counts: [usize; 2],
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
        Self::new(Duration::from_mins(1), 5, 20, 50)
    }
}

/// When a sweep of expired records may run ahead of [`EVICTION_INTERVAL`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sweep {
    /// Once the map has grown past [`EVICTION_MAP_THRESHOLD`].
    WhenLarge,
    /// At the first moment [`FULL_EVICTION_COOLDOWN`] allows.
    AsSoonAsAllowed,
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
                capped_counts: [0; 2],
                last_eviction: Instant::now(),
            }),
        }
    }

    fn evict_if_needed(&self, inner: &mut RateLimiterInner, now: Instant, sweep: Sweep) {
        let off_schedule = match sweep {
            Sweep::AsSoonAsAllowed => true,
            Sweep::WhenLarge => inner.map.len() > EVICTION_MAP_THRESHOLD,
        };
        let should_evict = now.duration_since(inner.last_eviction) >= EVICTION_INTERVAL
            || (off_schedule && now.duration_since(inner.last_eviction) >= FULL_EVICTION_COOLDOWN);
        if should_evict {
            let capped_counts = &mut inner.capped_counts;
            inner.map.retain(|key, record| {
                let active = now.duration_since(record.first_attempt) < self.window;
                if !active && let Some(pool) = key.capped_pool() {
                    let count = &mut capped_counts[pool as usize];
                    *count = count.saturating_sub(1);
                }
                active
            });
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
        self.evict_if_needed(&mut inner, now, Sweep::WhenLarge);

        if let Some(ip_address) = client_ip {
            self.check_key(
                &inner.map,
                &RateLimitKey::IpUser(ip_address, username.to_string()),
                self.max_ip_user_attempts,
                now,
            )?;
            self.check_key(&inner.map, &RateLimitKey::Ip(ip_address), self.max_ip_attempts, now)?;
        }
        let max_user_attempts = match client_ip {
            Some(_) => self.max_user_attempts,
            None => self.max_user_attempts.min(self.max_ip_user_attempts),
        };
        self.check_key(
            &inner.map,
            &RateLimitKey::User(username.to_string()),
            max_user_attempts,
            now,
        )?;

        Ok(())
    }

    fn update_existing_web_login(
        &self,
        record: &mut AttemptRecord,
        now: Instant,
    ) -> Result<(), u64> {
        if now.duration_since(record.first_attempt) >= self.window {
            record.count = 1;
            record.first_attempt = now;
            return Ok(());
        }
        if record.count >= self.max_ip_attempts {
            let elapsed = now.duration_since(record.first_attempt);
            let retry_after = self.window
                .saturating_sub(elapsed)
                .as_secs()
                .max(1);
            return Err(retry_after);
        }
        record.count += 1;
        Ok(())
    }

    fn ensure_web_login_capacity(
        &self,
        inner: &mut RateLimiterInner,
        pool: CappedPool,
        now: Instant,
    ) -> Result<(), u64> {
        if inner.capped_counts[pool as usize] < MAX_WEB_LOGIN_RECORDS {
            return Ok(());
        }
        self.evict_if_needed(inner, now, Sweep::AsSoonAsAllowed);
        if inner.capped_counts[pool as usize] < MAX_WEB_LOGIN_RECORDS {
            return Ok(());
        }
        let cooldown_active = now.duration_since(inner.last_eviction) < FULL_EVICTION_COOLDOWN;
        let has_expired = cooldown_active
            && inner.map
                .iter()
                .any(|(k, r)| {
                    k.capped_pool() == Some(pool)
                        && now.duration_since(r.first_attempt) >= self.window
                });
        let retry_after = if has_expired {
            FULL_EVICTION_COOLDOWN
                .saturating_sub(now.duration_since(inner.last_eviction))
                .as_secs()
                .max(1)
        } else {
            self.window.as_secs().max(1)
        };
        Err(retry_after)
    }

    /// Check and record a web login session creation attempt.
    /// Uses `peer_ip` as a fallback when `client_ip` is `None` (such as when
    /// a trusted proxy forwards an invalid header), so distinct socket peers
    /// do not share an anonymous bucket.
    /// Returns `Err(retry_after_seconds)` if rate limited or over capacity.
    pub(crate) fn check_and_record_web_login(
        &self,
        client_ip: Option<IpAddr>,
        peer_ip: Option<IpAddr>,
    ) -> Result<(), u64> {
        self.check_and_record_capped(
            RateLimitKey::WebLogin(client_ip.or(peer_ip)),
            CappedPool::WebLogin,
        )
    }

    /// Reserve one web login code submission for the client, identified as in
    /// [`Self::check_and_record_web_login`]. A submission whose code turns
    /// out to be valid gives its reservation back with
    /// [`Self::refund_web_login_code`], so only wrong codes use up the limit,
    /// and people behind one address do not throttle each other's sign-ins.
    pub(crate) fn reserve_web_login_code(
        &self,
        client_ip: Option<IpAddr>,
        peer_ip: Option<IpAddr>,
    ) -> Result<(), u64> {
        self.check_and_record_capped(
            RateLimitKey::WrongWebLoginCode(client_ip.or(peer_ip)),
            CappedPool::WrongWebLoginCode,
        )
    }

    /// Give back a reservation of [`Self::reserve_web_login_code`]. A record
    /// left with no attempts is removed, so valid sign-ins do not fill the
    /// pool.
    pub(crate) fn refund_web_login_code(&self, client_ip: Option<IpAddr>, peer_ip: Option<IpAddr>) {
        let mut inner = self.records.lock().unwrap();
        let key = RateLimitKey::WrongWebLoginCode(client_ip.or(peer_ip));
        let Some(record) = inner.map.get_mut(&key) else {
            return;
        };
        if record.count > 1 {
            record.count -= 1;
            return;
        }
        inner.map.remove(&key);
        let count = &mut inner.capped_counts[CappedPool::WrongWebLoginCode as usize];
        *count = count.saturating_sub(1);
    }

    /// Count an attempt under `key`, which belongs to `pool`.
    fn check_and_record_capped(&self, key: RateLimitKey, pool: CappedPool) -> Result<(), u64> {
        let now = Instant::now();
        let mut inner = self.records.lock().unwrap();

        if let Some(record) = inner.map.get_mut(&key) {
            return self.update_existing_web_login(record, now);
        }

        self.ensure_web_login_capacity(&mut inner, pool, now)?;
        self.evict_if_needed(&mut inner, now, Sweep::WhenLarge);

        inner.map.insert(key, AttemptRecord { count: 1, first_attempt: now });
        inner.capped_counts[pool as usize] += 1;

        Ok(())
    }

    /// Record a failed login attempt for `client_ip` and `username`.
    pub(crate) fn record_failure(&self, client_ip: Option<IpAddr>, username: &str) {
        let now = Instant::now();
        let mut inner = self.records.lock().unwrap();
        self.evict_if_needed(&mut inner, now, Sweep::WhenLarge);

        let mut keys = vec![RateLimitKey::User(username.to_string())];
        if let Some(ip_address) = client_ip {
            keys.push(RateLimitKey::IpUser(ip_address, username.to_string()));
            keys.push(RateLimitKey::Ip(ip_address));
        }

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
        }
        inner.map.remove(&RateLimitKey::User(username.to_string()));
    }
}
