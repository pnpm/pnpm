use super::LatencyProxy;
use std::{
    io::{Read as _, Write as _},
    net::{TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};

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

    let one_way = Duration::from_millis(60);
    let proxy = LatencyProxy::spawn(upstream_addr, one_way).expect("spawn proxy");

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
