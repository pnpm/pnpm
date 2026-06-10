use super::{LatencyProxy, LinkProfile, mbps_to_bytes_per_sec};
use std::{
    io::{Read as _, Write as _},
    net::{TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};

#[test]
fn binds_to_requested_listen_addr() {
    let upstream = TcpListener::bind(("127.0.0.1", 0)).expect("bind upstream");
    let upstream_addr = upstream.local_addr().expect("upstream addr");
    thread::spawn(move || {
        let (mut socket, _) = upstream.accept().expect("accept");
        let mut buf = [0u8; 64];
        let read = socket.read(&mut buf).expect("read request");
        assert_eq!(&buf[..read], b"ping");
        socket.write_all(b"pong").expect("write reply");
    });

    let reserved = TcpListener::bind(("127.0.0.1", 0)).expect("reserve listen port");
    let listen = reserved.local_addr().expect("listen addr");
    drop(reserved);

    let profile = LinkProfile { one_way: Duration::ZERO, rate_limit: None, slow_start: false };
    let proxy = LatencyProxy::spawn_on(listen, upstream_addr, profile).expect("spawn proxy");
    assert_eq!(proxy.addr, listen);

    let mut client = TcpStream::connect(proxy.addr).expect("connect to proxy");
    client.write_all(b"ping").expect("send request");
    let mut reply = [0u8; 64];
    let read = client.read(&mut reply).expect("read reply");
    assert_eq!(&reply[..read], b"pong");
}

#[test]
fn converts_mbps_to_bytes_per_second() {
    assert_eq!(mbps_to_bytes_per_sec(0.0), None);
    assert_eq!(mbps_to_bytes_per_sec(f64::NAN), None);
    assert_eq!(mbps_to_bytes_per_sec(8.0), Some(1_000_000));
    assert_eq!(mbps_to_bytes_per_sec(f64::MIN_POSITIVE), Some(1));
}

/// A request → response exchange through the proxy pays the one-way
/// delay in each direction, so its round trip is ≈ `2 × one_way`.
#[test]
fn injects_round_trip_latency() {
    // Upstream: read the request, echo a fixed reply.
    let upstream = TcpListener::bind(("127.0.0.1", 0)).expect("bind upstream");
    let upstream_addr = upstream.local_addr().expect("upstream addr");
    thread::spawn(move || {
        let (mut socket, _) = upstream.accept().expect("accept");
        let mut buf = [0u8; 64];
        let read = socket.read(&mut buf).expect("read request");
        assert_eq!(&buf[..read], b"ping");
        socket.write_all(b"pong").expect("write reply");
    });

    let profile =
        LinkProfile { one_way: Duration::from_millis(60), rate_limit: None, slow_start: false };
    let proxy = LatencyProxy::spawn(upstream_addr, profile).expect("spawn proxy");

    let mut client = TcpStream::connect(proxy.addr).expect("connect to proxy");
    let start = Instant::now();
    client.write_all(b"ping").expect("send request");
    let mut reply = [0u8; 64];
    let read = client.read(&mut reply).expect("read reply");
    let elapsed = start.elapsed();

    assert_eq!(&reply[..read], b"pong");
    // ≈ 120 ms expected; assert the delay is clearly present without
    // pinning an exact figure (scheduling adds slack on the high side).
    assert!(
        elapsed >= Duration::from_millis(100),
        "round trip {elapsed:?} should reflect the injected ~120ms latency",
    );
}

/// A bulk transfer through the proxy can't drain faster than the
/// bandwidth cap: 256 KiB at 1 MB/s takes at least ~0.25 s, where the
/// same transfer on loopback would finish in well under a millisecond.
#[test]
fn caps_throughput_to_the_rate_limit() {
    const PAYLOAD: usize = 256 * 1024;
    const RATE: u64 = 1_000_000; // 1 MB/s

    // Upstream: ignore the request, stream a fixed-size payload back.
    let upstream = TcpListener::bind(("127.0.0.1", 0)).expect("bind upstream");
    let upstream_addr = upstream.local_addr().expect("upstream addr");
    thread::spawn(move || {
        let (mut socket, _) = upstream.accept().expect("accept");
        let mut scratch = [0u8; 64];
        let _ = socket.read(&mut scratch);
        socket.write_all(&vec![0u8; PAYLOAD]).expect("write payload");
    });

    // No latency, only a bandwidth cap, so the wall time is the
    // serialization delay alone.
    let profile =
        LinkProfile { one_way: Duration::ZERO, rate_limit: Some(RATE), slow_start: false };
    let proxy = LatencyProxy::spawn(upstream_addr, profile).expect("spawn proxy");

    let mut client = TcpStream::connect(proxy.addr).expect("connect to proxy");
    let start = Instant::now();
    client.write_all(b"go").expect("send request");
    let mut received = Vec::with_capacity(PAYLOAD);
    let mut buf = [0u8; 16 * 1024];
    loop {
        match client.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => received.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
    }
    let elapsed = start.elapsed();

    assert_eq!(received.len(), PAYLOAD, "the whole payload should arrive");
    // 256 KiB / 1 MB/s ≈ 0.262 s. Allow generous slack below to absorb
    // scheduling jitter while still proving the cap throttled the stream
    // (loopback alone would be sub-millisecond).
    assert!(
        elapsed >= Duration::from_millis(200),
        "transfer {elapsed:?} should reflect the ~0.26s bandwidth cap",
    );
}

/// With slow start the same transfer takes the ramp-up rounds a real
/// TCP connection would: the early windows serialize at `cwnd ÷ RTT`,
/// far below the cap, so the wall time exceeds the flat-rate model's
/// `size ÷ cap` by several round trips.
#[test]
fn slow_start_ramps_per_connection_throughput() {
    const PAYLOAD: usize = 256 * 1024;
    const RATE: u64 = 10_000_000; // 10 MB/s cap

    let timed_transfer = |slow_start: bool| {
        let upstream = TcpListener::bind(("127.0.0.1", 0)).expect("bind upstream");
        let upstream_addr = upstream.local_addr().expect("upstream addr");
        thread::spawn(move || {
            let (mut socket, _) = upstream.accept().expect("accept");
            let mut buf = [0u8; 64];
            let _ = socket.read(&mut buf).expect("read request");
            socket.write_all(&vec![0u8; PAYLOAD]).expect("write payload");
        });

        let profile =
            LinkProfile { one_way: Duration::from_millis(20), rate_limit: Some(RATE), slow_start };
        let proxy = LatencyProxy::spawn(upstream_addr, profile).expect("spawn proxy");

        let mut client = TcpStream::connect(proxy.addr).expect("connect to proxy");
        let start = Instant::now();
        client.write_all(b"go").expect("send request");
        let mut received = 0;
        let mut buf = [0u8; 16 * 1024];
        loop {
            match client.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => received += n,
                Err(_) => break,
            }
        }
        assert_eq!(received, PAYLOAD, "the whole payload should arrive");
        start.elapsed()
    };

    let flat = timed_transfer(false);
    let ramped = timed_transfer(true);
    dbg!(flat, ramped);
    // Flat: ~2×20ms latency + 256KiB/10MB/s ≈ 66 ms. Ramped: the first
    // windows (14.6 KB and doubling) each serialize at cwnd/RTT, adding ~3-4
    // window-times before the rate approaches the cap. Require a solid
    // margin rather than exact math to stay robust under CI jitter.
    assert!(
        ramped > flat + Duration::from_millis(60),
        "slow start should add ramp-up time: flat {flat:?} vs ramped {ramped:?}",
    );
}
