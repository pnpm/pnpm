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
                .check_and_record_web_login(Some(ip_address), None)
                .is_ok(),
        );
    }

    // Exceeding threshold for IP
    assert!(
        limiter
            .check_and_record_web_login(Some(ip_address), None)
            .is_err(),
    );

    // When IP is None, it is tracked in its own bucket and still under threshold
    for _ in 0..3 {
        assert!(limiter.check_and_record_web_login(None, None).is_ok());
    }
    // Exceeding threshold for None
    assert!(limiter.check_and_record_web_login(None, None).is_err());
}

#[test]
fn web_login_falls_back_to_peer_ip_when_client_ip_is_none() {
    let limiter = LoginRateLimiter::new(Duration::from_mins(1), 5, 2, 5);
    let peer_a = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
    let peer_b = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2));

    assert!(
        limiter
            .check_and_record_web_login(None, Some(peer_a))
            .is_ok(),
    );
    assert!(
        limiter
            .check_and_record_web_login(None, Some(peer_a))
            .is_ok(),
    );
    // Peer A is now throttled
    assert!(
        limiter
            .check_and_record_web_login(None, Some(peer_a))
            .is_err(),
    );

    // Peer B has its own bucket and is not throttled
    assert!(
        limiter
            .check_and_record_web_login(None, Some(peer_b))
            .is_ok(),
    );
}

#[test]
fn web_login_rejects_new_sessions_when_capacity_reached() {
    let limiter = LoginRateLimiter::new(Duration::from_mins(1), 5, 2, 5);

    // Fill capacity up to MAX_WEB_LOGIN_RECORDS (1024)
    for index in 0..1024u32 {
        let ip = IpAddr::V4(Ipv4Addr::from(index + 1));
        assert!(
            limiter
                .check_and_record_web_login(Some(ip), None)
                .is_ok(),
            "IP #{index} should be accepted",
        );
    }

    // 1025th distinct IP is rejected due to capacity limit
    let over_capacity_ip = IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1));
    assert!(
        limiter
            .check_and_record_web_login(Some(over_capacity_ip), None)
            .is_err(),
        "new IP must be rejected when capacity is full",
    );

    // Already-admitted IP under its attempt limit is still accepted
    let existing_ip = IpAddr::V4(Ipv4Addr::from(1u32));
    assert!(
        limiter
            .check_and_record_web_login(Some(existing_ip), None)
            .is_ok(),
        "already-admitted IP should still be allowed under attempt limit",
    );
}

#[test]
fn rate_limiter_caps_account_attempts_across_client_ips() {
    let limiter = LoginRateLimiter::new(Duration::from_mins(1), 5, 20, 3);
    for suffix in 1..=3 {
        let client_ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, suffix));
        assert!(
            limiter
                .check(Some(client_ip), "alice")
                .is_ok(),
        );
        limiter.record_failure(Some(client_ip), "alice");
    }
    let fresh_ip = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 4));
    assert!(
        limiter
            .check(Some(fresh_ip), "alice")
            .is_err(),
    );
    assert!(limiter.check(None, "alice").is_err());
    assert!(
        limiter
            .check(Some(fresh_ip), "bob")
            .is_ok(),
    );
    limiter.record_success(Some(fresh_ip), "alice");
    assert!(
        limiter
            .check(Some(fresh_ip), "alice")
            .is_ok(),
    );
}

#[test]
fn one_client_ip_cannot_exhaust_the_default_account_limit() {
    let limiter = LoginRateLimiter::default();
    let attacker = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1));
    for _ in 0..5 {
        assert!(
            limiter
                .check(Some(attacker), "alice")
                .is_ok(),
        );
        limiter.record_failure(Some(attacker), "alice");
    }
    assert!(
        limiter
            .check(Some(attacker), "alice")
            .is_err(),
    );
    let owner = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 2));
    assert!(
        limiter
            .check(Some(owner), "alice")
            .is_ok(),
    );
}

#[test]
fn distributed_failures_exhaust_the_default_account_limit() {
    let limiter = LoginRateLimiter::default();
    for suffix in 1..=10 {
        let client = IpAddr::V4(Ipv4Addr::new(192, 0, 2, suffix));
        for _ in 0..5 {
            assert!(
                limiter
                    .check(Some(client), "alice")
                    .is_ok(),
            );
            limiter.record_failure(Some(client), "alice");
        }
    }
    let next = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 11));
    assert!(
        limiter
            .check(Some(next), "alice")
            .is_err(),
    );
    assert!(limiter.check(Some(next), "bob").is_ok());
}

#[test]
fn refunded_code_submissions_do_not_use_up_the_limit() {
    let limiter = LoginRateLimiter::new(Duration::from_mins(1), 5, 3, 5);
    let ip = Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
    for _ in 0..10 {
        assert!(limiter.reserve_web_login_code(ip, None).is_ok());
        limiter.refund_web_login_code(ip, None);
    }
    for _ in 0..3 {
        assert!(limiter.reserve_web_login_code(ip, None).is_ok());
    }
    assert!(limiter.reserve_web_login_code(ip, None).is_err());
    assert!(limiter.check_and_record_web_login(ip, None).is_ok(), "the start limit is separate");
}

#[test]
fn wrong_code_records_share_the_web_login_capacity() {
    let limiter = LoginRateLimiter::new(Duration::from_mins(1), 5, 2, 5);
    for index in 0..1024u32 {
        let ip = IpAddr::V4(Ipv4Addr::from(index + 1));
        assert!(
            limiter
                .reserve_web_login_code(Some(ip), None)
                .is_ok(),
            "IP #{index}",
        );
    }
    let over_capacity_ip = Some(IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1)));
    assert!(limiter.reserve_web_login_code(over_capacity_ip, None).is_err());
    assert!(limiter.check_and_record_web_login(over_capacity_ip, None).is_err());
}
