//! A network-link-emulating TCP proxy, used to front a pnpr server or the
//! registry so the benchmark measures them as the *remote* services they
//! are in production rather than loopback peers.
//!
//! Two costs are invisible on localhost but dominate once a service is
//! across a real network, so the proxy reintroduces both
//! ([`LinkProfile`]):
//!
//! * **Latency** — a chunk read at time `t` is forwarded no earlier than
//!   `t + one_way`, in each direction independently. A request → response
//!   exchange pays the delay twice (one round trip); a single large
//!   transfer pays it once at the front and then streams.
//! * **Bandwidth** — with a `rate_limit`, each direction serializes bytes
//!   at no more than the cap, so a tarball download takes
//!   `≈ one_way + size / rate`. Without it, loopback throughput (~GB/s)
//!   makes every download effectively free, which hides exactly the cost
//!   the public npm registry imposes (measured ~20-25 MB/s peak, and far
//!   less on a typical link) — and with it the fetch-overlaps-resolution
//!   win has nothing to overlap.
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

/// The emulated link applied to each direction of a proxied connection.
#[derive(Clone, Copy)]
pub struct LinkProfile {
    /// One-way propagation delay. A round trip pays it twice.
    pub one_way: Duration,
    /// Per-direction throughput cap in bytes per second; `None` leaves
    /// the direction at loopback speed (unlimited).
    pub rate_limit: Option<u64>,
}

/// A running proxy. Drop stops accepting new connections and joins the
/// accept thread; the benchmark holds it alongside the pnpr server guard
/// for the life of the run.
pub struct LatencyProxy {
    pub addr: SocketAddr,
    stop: Arc<AtomicBool>,
    accept_thread: Option<JoinHandle<()>>,
}

impl LatencyProxy {
    /// Front `upstream` with a proxy that applies `profile` to each
    /// direction. Returns the local address callers should connect to
    /// instead of `upstream`.
    pub fn spawn(upstream: SocketAddr, profile: LinkProfile) -> std::io::Result<LatencyProxy> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;

        let stop = Arc::new(AtomicBool::new(false));
        let accept_stop = Arc::clone(&stop);
        let accept_thread =
            thread::spawn(move || accept_loop(&listener, upstream, profile, &accept_stop));

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
    profile: LinkProfile,
    stop: &AtomicBool,
) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((inbound, _)) => {
                thread::spawn(move || handle_connection(inbound, upstream, profile));
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

/// Bridge one accepted connection to `upstream`, applying `profile` to
/// both directions. Runs the server→client pump on this thread and the
/// client→server pump on a spawned one, so the connection's threads end
/// together when either side closes.
fn handle_connection(inbound: TcpStream, upstream: SocketAddr, profile: LinkProfile) {
    let Ok(outbound) = TcpStream::connect(upstream) else { return };
    let _ = inbound.set_nodelay(true);
    let _ = outbound.set_nodelay(true);

    let (Ok(client_read), Ok(server_write)) = (inbound.try_clone(), outbound.try_clone()) else {
        return;
    };

    let up = thread::spawn(move || pump(client_read, server_write, profile));
    pump(outbound, inbound, profile);
    let _ = up.join();
}

/// Copy `src` → `dst`, applying the link `profile`: each chunk is held
/// back until `one_way` has elapsed since it was read (latency), and —
/// when a `rate_limit` is set — chunks are paced so the direction never
/// transmits faster than the cap (bandwidth). A reader thread stamps and
/// queues chunks so the source is never blocked by the delay; the writer
/// tracks when the link next frees up so a back-to-back burst is spread
/// at the cap rate instead of flushed at loopback speed.
fn pump(mut src: TcpStream, mut dst: TcpStream, profile: LinkProfile) {
    let (tx, rx) = mpsc::channel::<(Instant, Vec<u8>)>();

    let reader = thread::spawn(move || {
        let mut buf = vec![0u8; 32 * 1024];
        loop {
            match src.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let release = Instant::now() + profile.one_way;
                    if tx.send((release, buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Earliest instant the link is free to begin the next chunk. Advances
    // by each chunk's serialization time (`len / rate`) so the cap is a
    // sustained throughput limit, not just a per-chunk cap.
    let mut link_free_at = Instant::now();
    while let Ok((release, bytes)) = rx.recv() {
        // A chunk leaves no earlier than its latency release *and* no
        // earlier than the link finishing the previous chunk.
        let send_at = release.max(link_free_at);
        let now = Instant::now();
        if send_at > now {
            thread::sleep(send_at - now);
        }
        if let Some(rate) = profile.rate_limit {
            link_free_at = send_at + Duration::from_secs_f64(bytes.len() as f64 / rate as f64);
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
