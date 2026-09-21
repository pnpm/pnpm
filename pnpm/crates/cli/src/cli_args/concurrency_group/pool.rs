use super::{
    GroupStatus, HolderLine, WaiterLine,
    stamp::{elapsed_from_mtime, elapsed_since, parse_process_stamp, process_stamp},
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_millis(100);
const WAIT_NOTICE_EVERY: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub(super) struct SlotPool {
    pub(super) dir: PathBuf,
    pub(super) limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WaitSnapshot {
    pub(super) position: usize,
    pub(super) total: usize,
    pub(super) holders: Vec<String>,
    pub(super) ahead: Vec<String>,
}

struct Waiter {
    file: Option<File>,
    lock_path: PathBuf,
    stamp_path: PathBuf,
    ticket: u64,
    command: String,
}

struct LiveWaiter {
    ticket: u64,
    priority: i32,
    limit: u32,
    info: String,
    command: Option<String>,
    elapsed: Option<Duration>,
}

impl SlotPool {
    /// `None` once `cancelled` says so, checked before every attempt.
    pub(super) fn acquire(
        &self,
        command: &str,
        priority: i32,
        mut on_wait: impl FnMut(&WaitSnapshot),
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<Option<File>> {
        fs::create_dir_all(&self.dir)?;
        let mut waiter = self.enqueue(command, priority)?;
        let mut last_notice: Option<Instant> = None;
        loop {
            if cancelled() {
                return Ok(None);
            }
            if let Some(file) = self.try_take_turn(&mut waiter)? {
                return Ok(Some(file));
            }
            self.emit_wait_notice(&waiter, &mut last_notice, &mut on_wait)?;
            std::thread::sleep(POLL);
        }
    }

    fn try_take_turn(&self, waiter: &mut Waiter) -> io::Result<Option<File>> {
        let _seq = self.lock_seq()?;
        let Some(file) = self.try_acquire_if_eligible(waiter.ticket, &waiter.command)? else {
            return Ok(None);
        };
        waiter.leave_queue();
        Ok(Some(file))
    }

    /// Take a free slot that nobody ahead in line can use.
    fn try_acquire_if_eligible(&self, ticket: u64, command: &str) -> io::Result<Option<File>> {
        let waiters = self.live_waiters()?;
        let Some(position) = waiters
            .iter()
            .position(|waiter| waiter.ticket == ticket)
        else {
            return Ok(None);
        };
        let ahead = &waiters[..position];
        for index in 0..self.limit {
            if ahead
                .iter()
                .any(|waiter| waiter.limit > index)
            {
                continue;
            }
            if let Some(file) = self.try_lock_slot(index, command)? {
                return Ok(Some(file));
            }
        }
        Ok(None)
    }

    fn emit_wait_notice(
        &self,
        waiter: &Waiter,
        last_notice: &mut Option<Instant>,
        on_wait: &mut impl FnMut(&WaitSnapshot),
    ) -> io::Result<()> {
        if last_notice.is_some_and(|at| at.elapsed() < WAIT_NOTICE_EVERY) {
            return Ok(());
        }
        let snapshot = {
            let _seq = self.lock_seq()?;
            self.snapshot(waiter.ticket)?
        };
        on_wait(&snapshot);
        *last_notice = Some(Instant::now());
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn try_acquire(&self) -> io::Result<Option<File>> {
        for index in 0..self.limit {
            if let Some(file) = self.try_lock_slot(index, "")? {
                return Ok(Some(file));
            }
        }
        Ok(None)
    }

    fn try_lock_slot(&self, index: u32, command: &str) -> io::Result<Option<File>> {
        let file = self.open_slot(index)?;
        match file.try_lock() {
            Ok(()) => {
                if self.write_holder(index, command).is_err() {
                    let _ = fs::remove_file(self.holder_path(index));
                }
                Ok(Some(file))
            }
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(error)) => Err(error),
        }
    }

    fn open_slot(&self, index: u32) -> io::Result<File> {
        open_lock_file(&self.dir.join(index.to_string()))
    }

    fn holder_path(&self, index: u32) -> PathBuf {
        self.dir.join(format!("{index}.holder"))
    }

    fn write_holder(&self, index: u32, command: &str) -> io::Result<()> {
        fs::write(self.holder_path(index), process_stamp(command))
    }

    /// The stamps of the slots that are held right now. A slot whose lock
    /// this probe can take is free, and its stale stamp is left out.
    pub(super) fn holders(&self) -> Vec<String> {
        (0..self.limit)
            .filter_map(|index| self.holder_if_busy(index))
            .collect()
    }

    pub(super) fn status(&self) -> io::Result<GroupStatus> {
        let _seq = self.lock_seq()?;
        Ok(GroupStatus {
            holders: self
                .slot_indices()?
                .into_iter()
                .filter_map(|index| self.holder_line_if_busy(index))
                .collect(),
            waiters: self
                .live_waiters()?
                .into_iter()
                .map(|waiter| WaiterLine {
                    command: waiter.command,
                    priority: waiter.priority,
                    info: waiter.info,
                    elapsed: waiter.elapsed,
                })
                .collect(),
        })
    }

    fn slot_indices(&self) -> io::Result<Vec<u32>> {
        let mut indices = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let name = entry?.file_name();
            let Some(name) = name.to_str() else { continue };
            if let Ok(index) = name.parse() {
                indices.push(index);
            }
        }
        indices.sort_unstable();
        Ok(indices)
    }

    fn holder_if_busy(&self, index: u32) -> Option<String> {
        let line = self.holder_line_if_busy(index)?;
        (!line.info.starts_with("slot ")).then_some(line.info)
    }

    fn holder_line_if_busy(&self, index: u32) -> Option<HolderLine> {
        let file = self.open_slot(index).ok()?;
        match file.try_lock() {
            Err(std::fs::TryLockError::WouldBlock) => {}
            Ok(()) | Err(std::fs::TryLockError::Error(_)) => return None,
        }
        let path = self.holder_path(index);
        let text = fs::read_to_string(&path).unwrap_or_default();
        let stamp = parse_process_stamp(&text);
        let elapsed = stamp.since.and_then(elapsed_since).or_else(|| elapsed_from_mtime(&path));
        let info = if stamp.info.is_empty() { format!("slot {index}") } else { stamp.info };
        Some(HolderLine { command: stamp.command, info, elapsed })
    }

    fn enqueue(&self, command: &str, priority: i32) -> io::Result<Waiter> {
        fs::create_dir_all(self.waiters_dir())?;
        let mut seq = self.lock_seq()?;
        loop {
            let ticket = self.next_ticket(&mut seq)?;
            if let Some(waiter) = self.create_waiter(ticket, command, priority)? {
                return Ok(waiter);
            }
            if ticket == u64::MAX {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "concurrency-group waiter tickets exhausted",
                ));
            }
        }
    }

    #[expect(
        clippy::verbose_file_reads,
        reason = "The counter is incremented on the already-locked seq handle."
    )]
    fn next_ticket(&self, seq: &mut File) -> io::Result<u64> {
        seq.seek(SeekFrom::Start(0))?;
        let mut buf = Vec::new();
        seq.read_to_end(&mut buf)?;
        let ticket = match std::str::from_utf8(&buf)
            .ok()
            .and_then(|text| text.trim().parse().ok())
        {
            Some(ticket) => ticket,
            None => self
                .waiter_tickets()?
                .into_iter()
                .max()
                .map_or(0, |ticket| ticket.saturating_add(1)),
        };
        let stored = ticket.saturating_add(1).to_string();
        seq.seek(SeekFrom::Start(0))?;
        seq.write_all(stored.as_bytes())?;
        seq.set_len(stored.len() as u64)?;
        Ok(ticket)
    }

    fn lock_seq(&self) -> io::Result<File> {
        let file = open_lock_file(&self.dir.join("seq"))?;
        file.lock()?;
        Ok(file)
    }

    fn waiters_dir(&self) -> PathBuf {
        self.dir.join("waiters")
    }

    fn waiter_lock_path(&self, ticket: u64) -> PathBuf {
        self.waiters_dir().join(ticket.to_string())
    }

    fn waiter_stamp_path(&self, ticket: u64) -> PathBuf {
        self.waiters_dir()
            .join(format!("{ticket}.stamp"))
    }

    fn create_waiter(
        &self,
        ticket: u64,
        command: &str,
        priority: i32,
    ) -> io::Result<Option<Waiter>> {
        let lock_path = self.waiter_lock_path(ticket);
        let file = open_lock_file(&lock_path)?;
        match file.try_lock() {
            Ok(()) => {}
            // A live waiter already owns this ticket. Blocking here would
            // keep the seq lock that waiter needs to take a slot.
            Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
        }
        let stamp_path = self.waiter_stamp_path(ticket);
        fs::write(
            &stamp_path,
            format!("priority {priority}\nlimit {}\n{}", self.limit, process_stamp(command)),
        )?;
        Ok(Some(Waiter {
            file: Some(file),
            lock_path,
            stamp_path,
            ticket,
            command: command.to_string(),
        }))
    }

    fn snapshot(&self, ticket: u64) -> io::Result<WaitSnapshot> {
        let waiters = self.live_waiters()?;
        let position = match waiters
            .iter()
            .position(|waiter| waiter.ticket == ticket)
        {
            Some(index) => index + 1,
            None => waiters.len().saturating_add(1),
        };
        let ahead = waiters
            .iter()
            .take(position.saturating_sub(1))
            .filter(|waiter| !waiter.info.is_empty())
            .map(|waiter| waiter.info.clone())
            .collect();
        Ok(WaitSnapshot { position, total: waiters.len(), holders: self.holders(), ahead })
    }

    fn live_waiters(&self) -> io::Result<Vec<LiveWaiter>> {
        let mut live = Vec::new();
        for ticket in self.waiter_tickets()? {
            if self.scavenge_if_dead(ticket)? {
                continue;
            }
            live.push(self.read_live(ticket));
        }
        live.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then(a.ticket.cmp(&b.ticket))
        });
        Ok(live)
    }

    fn waiter_tickets(&self) -> io::Result<Vec<u64>> {
        let entries = match fs::read_dir(self.waiters_dir()) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
            Ok(entries) => entries,
        };
        let mut tickets = Vec::new();
        for entry in entries {
            let name = entry?.file_name();
            let Some(name) = name.to_str() else { continue };
            let Ok(ticket) = name.parse::<u64>() else { continue };
            tickets.push(ticket);
        }
        Ok(tickets)
    }

    fn scavenge_if_dead(&self, ticket: u64) -> io::Result<bool> {
        let file = match open_lock_file(&self.waiter_lock_path(ticket)) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(error),
            Ok(file) => file,
        };
        match file.try_lock() {
            Err(std::fs::TryLockError::WouldBlock) => Ok(false),
            Ok(()) => {
                drop(file);
                self.remove_waiter(ticket);
                Ok(true)
            }
            Err(std::fs::TryLockError::Error(error)) => Err(error),
        }
    }

    fn remove_waiter(&self, ticket: u64) {
        let _ = fs::remove_file(self.waiter_lock_path(ticket));
        let _ = fs::remove_file(self.waiter_stamp_path(ticket));
    }

    fn read_live(&self, ticket: u64) -> LiveWaiter {
        let path = self.waiter_stamp_path(ticket);
        let text = fs::read_to_string(&path).unwrap_or_default();
        let mut lines = text.lines();
        let priority = lines
            .next()
            .and_then(|line| line.strip_prefix("priority "))
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let second = lines.next().unwrap_or("");
        let (limit, rest) = match second.strip_prefix("limit ") {
            Some(value) => {
                (value.parse().unwrap_or(u32::MAX), lines.collect::<Vec<_>>().join("\n"))
            }
            None => (
                u32::MAX,
                std::iter::once(second)
                    .chain(lines)
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        };
        let stamp = parse_process_stamp(&rest);
        let elapsed = stamp.since.and_then(elapsed_since).or_else(|| elapsed_from_mtime(&path));
        LiveWaiter { ticket, priority, limit, info: stamp.info, command: stamp.command, elapsed }
    }
}

impl Waiter {
    fn leave_queue(&mut self) {
        drop(self.file.take());
        let _ = fs::remove_file(&self.lock_path);
        let _ = fs::remove_file(&self.stamp_path);
    }
}

impl Drop for Waiter {
    fn drop(&mut self) {
        self.leave_queue();
    }
}

fn open_lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}
