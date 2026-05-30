use std::net::TcpListener;

/// Ask the OS for an unused TCP port on the loopback interface.
pub fn pick_unused_port() -> Option<u16> {
    TcpListener::bind("127.0.0.1:0").ok()?.local_addr().ok().map(|addr| addr.port())
}
