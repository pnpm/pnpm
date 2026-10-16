use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};

use super::LoginRateLimiter;

#[test]
fn rate_limiter_allows_attempts_under_threshold() {
    let limiter = LoginRateLimiter::new(Duration::from_mins(1), 3, 10, 5);
    let ip_address = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));

    assert!(
        limiter
            .check(Some(ip_address), "alice")
            .is_ok(),
    );
    limiter.record_failure(Some(ip_address), "alice");
    assert!(
        limiter
            .check(Some(ip_address), "alice")
            .is_ok(),
    );
    limiter.record_failure(Some(ip_address), "alice");
    assert!(
        limiter
            .check(Some(ip_address), "alice")
            .is_ok(),
    );
}

#[test]
fn rate_limiter_throttles_user_after_exceeding_threshold() {
    let limiter = LoginRateLimiter::new(Duration::from_mins(1), 3, 10, 5);
    let ip_address = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));

    for _ in 0..3 {
        assert!(
            limiter
                .check(Some(ip_address), "alice")
                .is_ok(),
        );
        limiter.record_failure(Some(ip_address), "alice");
    }

    let check_result = limiter.check(Some(ip_address), "alice");
    assert!(check_result.is_err(), "expected throttling after 3 failures");

    // Failures from IP A must not block the same username from IP B
    let other_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20));
    assert!(
        limiter
            .check(Some(other_ip), "alice")
            .is_ok(),
    );

    // Other user from same IP should still be allowed if under IP threshold
    assert!(
        limiter
            .check(Some(ip_address), "bob")
            .is_ok(),
    );
}

#[test]
fn rate_limiter_throttles_ip_after_exceeding_ip_threshold() {
    let limiter = LoginRateLimiter::new(Duration::from_mins(1), 5, 4, 10);
    let ip_address = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));

    for user_index in 0..4 {
        let username = format!("user_{user_index}");
        assert!(
            limiter
                .check(Some(ip_address), &username)
                .is_ok(),
        );
        limiter.record_failure(Some(ip_address), &username);
    }

    // Now IP limit of 4 is reached
    let check_result = limiter.check(Some(ip_address), "another_user");
    assert!(check_result.is_err(), "expected IP throttling after 4 failures across users");
}

#[test]
fn rate_limiter_resets_on_success() {
    let limiter = LoginRateLimiter::new(Duration::from_mins(1), 3, 10, 5);
    let ip_address = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10));

    limiter.record_failure(Some(ip_address), "alice");
    limiter.record_failure(Some(ip_address), "alice");
    limiter.record_success(Some(ip_address), "alice");

    // After success, previous failure count for alice is cleared
    limiter.record_failure(Some(ip_address), "alice");
    limiter.record_failure(Some(ip_address), "alice");
    assert!(
        limiter
            .check(Some(ip_address), "alice")
            .is_ok(),
    );
}

#[test]
fn rate_limiter_throttles_by_username_even_when_ip_is_none() {
    let limiter = LoginRateLimiter::new(Duration::from_mins(1), 5, 10, 2);
    assert!(limiter.check(None, "alice").is_ok());
    limiter.record_failure(None, "alice");
    assert!(limiter.check(None, "alice").is_ok());
    limiter.record_failure(None, "alice");
    assert!(limiter.check(None, "alice").is_err(), "expected throttling by username");
}

#[test]
fn web_login_throttles_after_exceeding_ip_threshold() {
    let limiter = LoginRateLimiter::new(Duration::from_mins(1), 5, 3, 5);
    let ip_address = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5));

    // Under threshold for IP
    for _ in 0..3 {
        assert!(
            limiter
                .check_and_record_web_login(Some(ip_address))
                .is_ok(),
        );
    }

    // Exceeding threshold for IP
    assert!(
        limiter
            .check_and_record_web_login(Some(ip_address))
            .is_err(),
    );

    // When IP is None, it is tracked in its own bucket and still under threshold
    for _ in 0..3 {
        assert!(limiter.check_and_record_web_login(None).is_ok());
    }
    // Exceeding threshold for None
    assert!(limiter.check_and_record_web_login(None).is_err());
}
