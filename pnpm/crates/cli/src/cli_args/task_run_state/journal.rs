use super::{
    FINISHED_SUFFIX, File, HashSet, IntoDiagnostic, JournalRecord, LOCK_ABANDONED_AFTER, LOCK_WAIT,
    OsStr, OsString, PUBLISHED_SUFFIX, Path, PathBuf, RUN_GENERATION_LENGTH, START_LOCK_DIR,
    STATE_VERSION, StateHeader, StateStorageError, SystemTime, TaskId, TaskKey,
    TaskRunStateContext, UNIX_EPOCH, fs, initial_journal_contents, io, is_state_unavailable_error,
    open_journal_for_append, run_id,
};
use miette::WrapErr as _;

/// Remove one state file. Reports `false` when the state directory has
/// become unavailable, in which case no later state write will land either.
pub(super) fn remove_state_file(path: &Path) -> miette::Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(error) if is_state_unavailable_error(&error) => Ok(false),
        Err(error) => {
            Err(error).into_diagnostic().wrap_err_with(|| format!("removing {}", path.display()))
        }
    }
}

fn is_run_id(run: &str) -> bool {
    run.len() > RUN_GENERATION_LENGTH + 1
        && run.len() <= 128
        && run.as_bytes()[RUN_GENERATION_LENGTH] == b'-'
        && run[..RUN_GENERATION_LENGTH]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && run[RUN_GENERATION_LENGTH + 1..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) || byte == b'-')
}

fn run_generation(run: &str) -> &str {
    &run[..RUN_GENERATION_LENGTH]
}

pub(super) fn validate_real_directory(
    path: &Path,
    create: bool,
) -> Result<bool, StateStorageError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound && !create => return Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match fs::create_dir(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(StateStorageError::io(error, "creating", path));
                }
            }
            fs::symlink_metadata(path)
                .map_err(|error| StateStorageError::io(error, "inspecting", path))?
        }
        Err(error) => {
            return Err(StateStorageError::io(error, "inspecting", path));
        }
    };
    if metadata.file_type().is_symlink()
        || pnpm_fs::read_symlink_dir(path).is_ok()
        || !metadata.is_dir()
    {
        return Err(StateStorageError::UnsafePath(path.to_path_buf()));
    }
    Ok(true)
}

pub(super) fn current_generation() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
}

impl TaskRunStateContext {
    /// The run recorded as the latest one, when it belongs to this
    /// invocation and its identifier is well-formed.
    pub(super) fn resumable_latest_run(&self) -> miette::Result<Option<String>> {
        let state_directory_exists = match self.validate_state_directory(false) {
            Ok(exists) => exists,
            Err(error) if error.is_unavailable() => return Ok(None),
            Err(error) => return Err(error.into_report()),
        };
        if !state_directory_exists {
            return Ok(None);
        }
        let latest = match fs::read_to_string(&self.latest_state_path) {
            Ok(contents) => serde_json::from_str::<StateHeader>(&contents).ok(),
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    || is_state_unavailable_error(&error) =>
            {
                return Ok(None);
            }
            Err(error) => {
                return Err(error)
                    .into_diagnostic()
                    .wrap_err_with(|| format!("reading {}", self.latest_state_path.display()));
            }
        };
        let Some(latest) = latest else { return Ok(None) };
        if latest.version != STATE_VERSION
            || latest.invocation != self.invocation
            || !is_run_id(&latest.run)
        {
            return Ok(None);
        }
        Ok(Some(latest.run))
    }

    /// The tasks `run`'s journal records as completed, or `None` when the
    /// journal cannot be trusted to describe this run.
    pub(super) fn completed_from_journal(
        &self,
        contents: &[u8],
        run: &str,
    ) -> Option<HashSet<TaskKey>> {
        // A record is committed by its newline; a process killed during
        // append can leave only the final record torn.
        let last_newline = contents.iter().rposition(|byte| *byte == b'\n')?;
        let complete = std::str::from_utf8(&contents[..last_newline]).ok()?;
        let mut lines = complete.lines();
        let header = serde_json::from_str::<StateHeader>(lines.next()?).ok()?;
        if header.version != STATE_VERSION
            || header.invocation != self.invocation
            || header.run != run
        {
            return None;
        }
        let mut completed = HashSet::new();
        for line in lines {
            let record = serde_json::from_str::<JournalRecord>(line).ok()?;
            match record {
                JournalRecord::Task(record) if record.run == header.run => {
                    let id = TaskId { project: record.project, task: record.task };
                    completed.insert(self.keys_by_id.get(&id)?.clone());
                }
                JournalRecord::Finish(record) if record.run == header.run && record.finished => {
                    return None;
                }
                _ => {}
            }
        }
        Some(completed)
    }

    pub(super) fn start_file(
        &self,
        completed: &[&TaskId],
    ) -> Result<(PathBuf, String, Option<File>), StateStorageError> {
        self.validate_state_directory(true)?;
        let lock_path = self.state_dir.join(START_LOCK_DIR);
        let Some(lock) =
            pnpm_fs::DirLock::acquire(lock_path.clone(), LOCK_WAIT, LOCK_ABANDONED_AFTER)
                .map_err(|error| StateStorageError::io(error, "locking", &lock_path))?
        else {
            let run = run_id(current_generation());
            return Ok((self.journal_path(&run), run, None));
        };
        let run = self.next_run_id()?;
        let header = StateHeader {
            version: STATE_VERSION,
            invocation: self.invocation.clone(),
            run: run.clone(),
        };
        let contents = initial_journal_contents(&header, completed);
        let file_path = self.journal_path(&run);
        pnpm_fs::write_atomic(&file_path, contents.as_bytes())
            .map_err(|error| StateStorageError::io(error, "writing", &file_path))?;
        let file = open_journal_for_append(&file_path)?;
        match lock.is_owner() {
            Ok(true) => {}
            Ok(false) => {
                drop(file);
                let _ = fs::remove_file(&file_path);
                return Ok((file_path, run, None));
            }
            Err(error) => {
                drop(file);
                let _ = fs::remove_file(&file_path);
                return Err(StateStorageError::io(error, "checking", &lock_path));
            }
        }
        self.publish_journal(&header, &file_path, file).map(|file| {
            self.cleanup_older_finished_state(&run);
            (file_path, run, Some(file))
        })
    }

    fn publish_journal(
        &self,
        header: &StateHeader,
        file_path: &Path,
        file: File,
    ) -> Result<File, StateStorageError> {
        let latest_write = pnpm_fs::write_atomic(
            &self.latest_state_path,
            serde_json::to_string(header).expect("latest task state serializes").as_bytes(),
        );
        if let Err(error) = latest_write {
            drop(file);
            let _ = fs::remove_file(file_path);
            return Err(StateStorageError::io(error, "writing", &self.latest_state_path));
        }
        let published_path = self.published_path(&header.run);
        if let Err(error) = pnpm_fs::write_atomic(&published_path, &[]) {
            drop(file);
            let _ = fs::remove_file(file_path);
            let _ = fs::remove_file(&published_path);
            return Err(StateStorageError::io(error, "writing", &published_path));
        }
        Ok(file)
    }

    pub(super) fn newest_state(
        &self,
        latest_run: &str,
    ) -> Result<(String, bool), StateStorageError> {
        let mut newest_run = latest_run.to_string();
        let prefix = format!("{}.", self.invocation);
        let entries = fs::read_dir(&self.state_dir)
            .map_err(|error| StateStorageError::io(error, "reading", &self.state_dir))?;
        let mut names = HashSet::new();
        for entry in entries {
            let entry =
                entry.map_err(|error| StateStorageError::io(error, "reading", &self.state_dir))?;
            names.insert(entry.file_name());
        }
        let mut finished =
            names.contains(OsStr::new(&format!("{prefix}{latest_run}{FINISHED_SUFFIX}")));
        for name in &names {
            let Some((run, candidate_finished)) = Self::state_file_run(name, &prefix, &names)
            else {
                continue;
            };
            if run_generation(run) > run_generation(&newest_run) {
                newest_run = run.to_string();
                finished = candidate_finished;
            } else if run == newest_run && candidate_finished {
                finished = true;
            }
        }
        Ok((newest_run, finished))
    }

    /// The run a state file name belongs to, and whether that name marks
    /// the run as finished. `None` for a name that does not belong to this
    /// invocation, or whose journal was never published.
    fn state_file_run<'name>(
        name: &'name OsString,
        prefix: &str,
        names: &HashSet<OsString>,
    ) -> Option<(&'name str, bool)> {
        let name = name.to_str()?.strip_prefix(prefix)?;
        if let Some(run) = name.strip_suffix(FINISHED_SUFFIX) {
            return is_run_id(run).then_some((run, true));
        }
        let run = name.strip_suffix(".jsonl")?;
        if !names.contains(OsStr::new(&format!("{prefix}{run}{PUBLISHED_SUFFIX}"))) {
            return None;
        }
        is_run_id(run).then_some((run, false))
    }

    fn next_run_id(&self) -> Result<String, StateStorageError> {
        let mut newest_run = run_id(current_generation());
        match fs::read_to_string(&self.latest_state_path) {
            Ok(contents) => {
                if let Ok(latest) = serde_json::from_str::<StateHeader>(&contents)
                    && latest.invocation == self.invocation
                    && is_run_id(&latest.run)
                    && run_generation(&latest.run) > run_generation(&newest_run)
                {
                    newest_run = latest.run;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(StateStorageError::io(error, "reading", &self.latest_state_path));
            }
        }
        newest_run = self.newest_state(&newest_run)?.0;
        let generation = u64::from_str_radix(run_generation(&newest_run), 16)
            .expect("validated run generation")
            .saturating_add(1);
        Ok(run_id(generation))
    }

    fn cleanup_older_finished_state(&self, run: &str) {
        let Ok(entries) = fs::read_dir(&self.state_dir) else { return };
        let prefix = format!("{}.", self.invocation);
        let generation = run_generation(run);
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(name) = name.strip_prefix(&prefix) else { continue };
            let Some(older_run) = name.strip_suffix(FINISHED_SUFFIX) else { continue };
            if !is_run_id(older_run) || run_generation(older_run) >= generation {
                continue;
            }
            let _ = fs::remove_file(entry.path());
        }
    }
}
