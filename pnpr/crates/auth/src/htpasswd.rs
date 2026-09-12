use super::{
    AtomicU64, HashMap, Ordering, Path, PathBuf, RegistryError, Result, UpsertOutcome,
    validate_username,
};

// ---------------------------------------------------------------
// htpasswd I/O
// ---------------------------------------------------------------

/// Parse an Apache-shaped htpasswd file. Each non-empty, non-comment
/// line is `username:hash`; we accept any bcrypt variant (`$2a$`,
/// `$2b$`, `$2y$`) but reject everything else so a config file
/// holding `crypt(3)` or plaintext entries can't masquerade as
/// passing without the password actually being verifiable.
pub(super) fn parse_htpasswd(raw: &str) -> std::result::Result<HashMap<String, String>, String> {
    let mut out = HashMap::new();
    for (line_no, line) in raw.lines().enumerate() {
        let line = line.trim_end_matches(['\r']);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (user, hash) =
            parse_htpasswd_line(line).map_err(|err| format!("line {}: {err}", line_no + 1))?;
        out.insert(user.to_string(), hash.to_string());
    }
    Ok(out)
}

/// The user and bcrypt hash one htpasswd line declares.
pub(super) fn parse_htpasswd_line(line: &str) -> std::result::Result<(&str, &str), String> {
    let Some((user, hash)) = line.split_once(':') else {
        return Err("missing ':' separator".to_string());
    };
    let user = user.trim();
    let hash = hash.trim();
    if user.is_empty() {
        return Err("empty username".to_string());
    }
    if let Err(err) = validate_username(user) {
        let reason = match err {
            RegistryError::BadRequest { reason } => reason,
            err => err.to_string(),
        };
        return Err(format!("invalid username {user:?}: {reason}"));
    }
    if !is_supported_hash(hash) {
        return Err(format!("unsupported hash format for user {user:?} (only bcrypt is accepted)"));
    }
    Ok((user, hash))
}

/// True for any bcrypt variant. We don't accept `{SHA}`, `$apr1$`,
/// crypt(3), or plaintext — every supported entry must go through
/// `bcrypt::verify` cleanly.
pub(super) fn is_supported_hash(hash: &str) -> bool {
    hash.starts_with("$2a$") || hash.starts_with("$2b$") || hash.starts_with("$2y$")
}

/// Serialize the user map back to htpasswd shape. Sorted output so
/// the file is stable under `git diff` and easier to eyeball.
pub(super) fn serialize_htpasswd(users: &HashMap<String, String>) -> String {
    let mut entries: Vec<(&String, &String)> = users.iter().collect();
    entries.sort_by(|left, right| left.0.cmp(right.0));
    let mut out = String::new();
    for (user, hash) in entries {
        out.push_str(user);
        out.push(':');
        out.push_str(hash);
        out.push('\n');
    }
    out
}

pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write as _;

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = unique_tmp_path(path);
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub(super) fn unique_tmp_path(base: &Path) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let mut name = base.file_name().map(std::ffi::OsStr::to_os_string).unwrap_or_default();
    name.push(format!(".tmp.{pid}.{counter}"));
    match base.parent() {
        Some(parent) => parent.join(name),
        None => PathBuf::from(name),
    }
}

// ---------------------------------------------------------------
// bcrypt helpers
// ---------------------------------------------------------------

/// Hash a password off the reactor — bcrypt at cost 10 takes
/// ~50–100 ms and stalls every other async task on the same thread
/// if run inline.
pub(super) async fn hash_bcrypt(password: String, cost: u32) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        let parts = bcrypt::hash_with_result(&password, cost)?;
        // Format as $2y$ for maximum cross-tool compatibility —
        // Apache's `htpasswd -B` writes $2y$, GNU coreutils tools
        // accept it, and bcrypt::verify reads any of $2a/$2b/$2y.
        Ok(parts.format_for_version(bcrypt::Version::TwoY))
    })
    .await?
}

pub(super) async fn verify_bcrypt(password: String, hash: String) -> Result<bool> {
    tokio::task::spawn_blocking(move || {
        bcrypt::verify(&password, &hash).map_err(RegistryError::from)
    })
    .await?
}

/// Verify `password` against an existing user's `stored` hash,
/// mapping the result to the login outcome a returning user expects:
/// `LoggedIn` on a match, `Unauthenticated` otherwise.
pub(super) async fn verify_returning_user(
    username: &str,
    password: &str,
    stored: String,
) -> Result<(UpsertOutcome, String)> {
    if verify_bcrypt(password.to_string(), stored).await? {
        Ok((UpsertOutcome::LoggedIn, username.to_string()))
    } else {
        Err(RegistryError::Unauthenticated { resource: format!("user {username:?}") })
    }
}
