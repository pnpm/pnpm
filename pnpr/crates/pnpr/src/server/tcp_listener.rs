use super::{Connected, IncomingStream, SocketAddr};

/// Wraps [`tokio::net::TcpListener`] to disable Nagle's algorithm on
/// every accepted socket.
///
/// Node's http server sets `TCP_NODELAY` by default; hyper 1.x
/// doesn't. With Nagle on, the kernel coalesces small writes and
/// (on Linux epoll) introduces ~tens-of-µs of per-response delay
/// while waiting for follow-up bytes that never come — invisible
/// on macOS's kqueue scheduling, but stacks up across the
/// thousand-request fan-out of an install benchmark.
///
/// Set on a per-socket basis after accept because the option lives
/// on the *connection*, not the listening socket.
pub(super) struct NodelayTcpListener(pub(super) tokio::net::TcpListener);

impl axum::serve::Listener for NodelayTcpListener {
    type Io = tokio::net::TcpStream;
    type Addr = std::net::SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match self.0.accept().await {
                Ok((socket, addr)) => {
                    // Ignore set_nodelay errors — failure means the
                    // peer already closed; serving the connection
                    // will surface that as a normal HTTP error.
                    let _ = socket.set_nodelay(true);
                    return (socket, addr);
                }
                Err(err) => {
                    tracing::warn!(?err, "tcp accept error; retrying");
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.0.local_addr()
    }
}

/// Client socket address captured from the accepted TCP connection, for
/// the CIDR-restriction gate. A local newtype (rather than [`SocketAddr`]
/// directly) so we can implement axum's [`Connected`] for
/// [`NodelayTcpListener`] — the blanket impl axum ships covers only the
/// bare [`tokio::net::TcpListener`], not our wrapper. This is the real
/// peer address from the socket, never a client-supplied forwarding
/// header, so it can't be spoofed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PeerAddr(pub(crate) SocketAddr);

impl Connected<IncomingStream<'_, NodelayTcpListener>> for PeerAddr {
    fn connect_info(stream: IncomingStream<'_, NodelayTcpListener>) -> Self {
        PeerAddr(*stream.remote_addr())
    }
}
