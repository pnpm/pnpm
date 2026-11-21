//! Restarting a benchmark's pnpr servers with an empty cache between the
//! installs it measures.

use super::{
    PnprResolverServer,
    WorkEnv,
    remove_dir_all_with_retry,
    servers::PnprServerLogs,
    wait_for_pnpr_ready,
};
use std::{
    io::{
        BufRead,
        BufReader,
        Write,
    },
    net::{
        TcpListener,
        TcpStream,
    },
    sync::atomic::{
        AtomicBool,
        Ordering,
    },
    thread,
    time::Duration,
};

impl WorkEnv {
    /// Restart a pnpr resolver server with an empty cache. Killing the process
    /// is what drops the in-memory resolution cache: emptying the cache
    /// directory under a live server leaves it warm.
    pub(super) fn restart_pnpr_server_with_empty_cache(&self, resolver: &mut PnprResolverServer) {
        let PnprResolverServer { server, paths } = resolver;
        server.process.kill().expect("stop the pnpr server before restarting it");
        server.process.wait().expect("wait for the stopped pnpr server to exit");
        remove_dir_all_with_retry(&paths.cache()).expect("empty the pnpr cache before restart");
        server.process = self.spawn_pnpr_server_process(paths, PnprServerLogs::Append);
        wait_for_pnpr_ready(paths.port);
    }
}

/// The loopback socket through which hyperfine's `--prepare` asks the
/// benchmark to restart one of its pnpr servers. The restart runs in this
/// process, so every server stays a [`super::PnprServer`] guard that dies with the run.
pub(super) struct PnprRestartListener {
    listener: TcpListener,
    stopped: AtomicBool,
}
impl PnprRestartListener {
    const ACK: &'static str = "restarted";
    const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

    pub(super) fn bind() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind the pnpr restart listener");
        PnprRestartListener { listener, stopped: AtomicBool::new(false) }
    }

    /// A `--prepare` step that asks for `server` to be restarted. It blocks
    /// until the restart has finished and fails when no acknowledgement
    /// arrives, so an iteration never runs against a server that was not
    /// restarted.
    pub(super) fn prepare_command(&self, server: usize) -> String {
        let port = self.port();
        format!(
            r#"bash -c 'exec 3<>/dev/tcp/127.0.0.1/{port} && echo {server} >&3 && read -r reply <&3 && [ "$reply" = {ack} ]'"#,
            ack = Self::ACK,
        )
    }

    pub(super) fn port(&self) -> u16 {
        self.listener
            .local_addr()
            .expect("pnpr restart listener address")
            .port()
    }

    /// Serve restart requests on a scoped thread while `run` executes.
    /// `restart` receives the server index its [`Self::prepare_command`] named
    /// and returns whether that index names one of the servers.
    pub(super) fn serve_during(
        &self,
        restart: impl FnMut(usize) -> bool + Send,
        run: impl FnOnce(),
    ) {
        thread::scope(|scope| {
            scope.spawn(|| self.serve(restart));
            let _stop = StopOnDrop(self);
            run();
        });
    }

    fn serve(&self, mut restart: impl FnMut(usize) -> bool) {
        for stream in self.listener.incoming() {
            if self.stopped.load(Ordering::Acquire) {
                return;
            }
            let mut stream = stream.expect("accept a pnpr restart request");
            // Another local process may connect without naming a server. It
            // gets no acknowledgement, and the listener moves on.
            let Some(server) = Self::read_request(&stream) else { continue };
            if !restart(server) {
                continue;
            }
            writeln!(stream, "{}", Self::ACK).expect("acknowledge the pnpr restart");
        }
    }

    fn read_request(stream: &TcpStream) -> Option<usize> {
        stream
            .set_read_timeout(Some(Self::REQUEST_TIMEOUT))
            .expect("bound the pnpr restart request read");
        let mut request = String::new();
        BufReader::new(stream).read_line(&mut request).ok()?;
        request.trim().parse().ok()
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        let addr = self.listener.local_addr().expect("pnpr restart listener address");
        // Wakes the blocking accept in `serve`. Fails harmlessly once it has returned.
        let _ = TcpStream::connect(addr);
    }
}
/// Stops the listener even while a panic unwinds out of `run`, so the scope
/// in [`PnprRestartListener::serve_during`] can join its serving thread.
struct StopOnDrop<'a>(&'a PnprRestartListener);
impl Drop for StopOnDrop<'_> {
    fn drop(&mut self) {
        self.0.stop();
    }
}
