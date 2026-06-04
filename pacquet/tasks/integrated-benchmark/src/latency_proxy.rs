//! A latency-injecting TCP proxy, used to front a pnpr server so the
//! benchmark measures it as the *remote* service it is in production
//! rather than a loopback peer.
//!
//! pnpr's round-trip cost is invisible on localhost (RTT ≈ 0), which is
//! exactly the cost that matters once the server is across a network. The
//! proxy reintroduces it: a chunk read at time `t` is forwarded no
//! earlier than `t + one_way_delay`, in each direction independently. A
//! request → response exchange therefore pays the delay twice — one full
//! round trip — while a single large transfer pays it once at the front
//! and then streams, matching how TCP behaves over a link with latency.
//!
//! It is deliberately dependency-free (std threads + blocking sockets):
//! the integrated-benchmark orchestrator drives everything synchronously,
//! so a thread-per-direction proxy slots in without dragging an async
//! runtime into the benchmark path.

use std::{
    io::{Read as _, Write as _},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// A running proxy. Drop stops accepting new connections and joins the
/// accept thread; the benchmark holds it alongside the pnpr server guard
/// for the life of the run.
pub struct LatencyProxy {
    pub addr: SocketAddr,
    stop: Arc<AtomicBool>,
    accept_thread: Option<JoinHandle<()>>,
}

impl LatencyProxy {
    /// Front `upstream` with a proxy that delays each direction by
    /// `one_way_delay`. Returns the local address callers should connect
    /// to instead of `upstream`.
    pub fn spawn(upstream: SocketAddr, one_way_delay: Duration) -> std::io::Result<LatencyProxy> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;

        let stop = Arc::new(AtomicBool::new(false));
        let accept_stop = Arc::clone(&stop);
        let accept_thread =
            thread::spawn(move || accept_loop(&listener, upstream, one_way_delay, &accept_stop));

        Ok(LatencyProxy { addr, stop, accept_thread: Some(accept_thread) })
    }
}

impl Drop for LatencyProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.accept_thread.take() {
            let _ = handle.join();
        }
    }
}

fn accept_loop(
    listener: &TcpListener,
    upstream: SocketAddr,
    one_way_delay: Duration,
    stop: &AtomicBool,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((inbound, _)) => {
                thread::spawn(move || handle_connection(inbound, upstream, one_way_delay));
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            // A real accept error (listener closed) ends the proxy; the
            // benchmark is the only client and is already tearing down.
            Err(_) => break,
        }
    }
}

/// Bridge one accepted connection to `upstream`, delaying both
/// directions. Runs the server→client pump on this thread and the
/// client→server pump on a spawned one, so the connection's threads end
/// together when either side closes.
fn handle_connection(inbound: TcpStream, upstream: SocketAddr, one_way_delay: Duration) {
    let Ok(outbound) = TcpStream::connect(upstream) else { return };
    let _ = inbound.set_nodelay(true);
    let _ = outbound.set_nodelay(true);

    let (Ok(client_read), Ok(server_write)) = (inbound.try_clone(), outbound.try_clone()) else {
        return;
    };

    let up = thread::spawn(move || pump(client_read, server_write, one_way_delay));
    pump(outbound, inbound, one_way_delay);
    let _ = up.join();
}

/// Copy `src` → `dst`, holding each chunk back until `one_way_delay` has
/// elapsed since it was read. A reader thread stamps and queues chunks so
/// the source is never blocked by the delay — back-to-back chunks all
/// shift by the same delay and then stream out, rather than each paying
/// it serially.
fn pump(mut src: TcpStream, mut dst: TcpStream, one_way_delay: Duration) {
    let (tx, rx) = mpsc::channel::<(Instant, Vec<u8>)>();

    let reader = thread::spawn(move || {
        let mut buf = vec![0u8; 32 * 1024];
        loop {
            match src.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let release = Instant::now() + one_way_delay;
                    if tx.send((release, buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    });

    while let Ok((release, bytes)) = rx.recv() {
        let now = Instant::now();
        if release > now {
            thread::sleep(release - now);
        }
        if dst.write_all(&bytes).is_err() {
            break;
        }
    }

    // Drop the receiver before joining so that if we broke out early (a
    // write error), the reader's next `tx.send` fails and it stops
    // promptly, rather than buffering the rest of the source into a queue
    // no one drains while `join` blocks.
    drop(rx);
    let _ = dst.shutdown(Shutdown::Write);
    let _ = reader.join();
}

#[cfg(test)]
mod tests;
