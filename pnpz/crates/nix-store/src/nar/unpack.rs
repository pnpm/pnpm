use super::{
    MAGIC,
    STORE_MTIME_SECONDS,
};
use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use std::{
    fs::{
        self,
        File,
        FileTimes,
        OpenOptions,
        Permissions,
    },
    io::{
        self,
        Read,
    },
    os::unix::fs::{
        PermissionsExt,
        symlink,
    },
    path::{
        Path,
        PathBuf,
    },
    time::{
        Duration,
        SystemTime,
    },
};

const MAX_TOKEN_LEN: u64 = 32;
const MAX_NAME_LEN: u64 = 255;
const MAX_TARGET_LEN: u64 = 4095;
const MAX_DEPTH: usize = 128;

#[derive(Debug, Display, Error, Diagnostic)]
pub enum UnpackError {
    #[display("the NAR is malformed: {_0}")]
    #[diagnostic(code(ERR_PNPZ_NAR_MALFORMED))]
    Malformed(#[error(not(source))] String),
    #[display("failed to read the NAR: {_0}")]
    #[diagnostic(code(ERR_PNPZ_NAR_READ))]
    Read(io::Error),
    #[display("failed to write {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPZ_NAR_WRITE))]
    Write { path: PathBuf, source: io::Error },
}

/// Unpacks the archive in `reader` to `destination`, which must not exist.
///
/// The tree gets the metadata Nix gives a store path: files are `0444` or
/// `0555`, directories `0555`, and files and directories are dated one second
/// after the epoch. Symlinks keep the time they were created at.
///
/// The archive is not trusted. Entry names cannot leave `destination`, and
/// nothing is written through a symlink, because every entry is created new.
/// A caller that verifies a hash afterwards deletes `destination` on mismatch.
pub fn unpack(reader: impl Read, destination: &Path) -> Result<(), UnpackError> {
    let mut unpacker = Unpacker { input: reader };
    unpacker.expect(MAGIC)?;
    unpacker.node(destination, 0)
}

struct Unpacker<Input> {
    input: Input,
}

fn malformed<Value>(message: impl Into<String>) -> Result<Value, UnpackError> {
    Err(UnpackError::Malformed(message.into()))
}

fn write_error(path: &Path) -> impl FnOnce(io::Error) -> UnpackError + '_ {
    move |source| UnpackError::Write { path: path.to_path_buf(), source }
}

impl<Input: Read> Unpacker<Input> {
    fn integer(&mut self) -> Result<u64, UnpackError> {
        let mut bytes = [0_u8; 8];
        self.input.read_exact(&mut bytes).map_err(UnpackError::Read)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn skip_padding(&mut self, len: u64) -> Result<(), UnpackError> {
        let mut padding = [0_u8; 8];
        let padding = &mut padding[..(8 - (len % 8) as usize) % 8];
        self.input.read_exact(padding).map_err(UnpackError::Read)?;
        if padding.iter().any(|byte| *byte != 0) {
            return malformed("string padding is not zero");
        }
        Ok(())
    }

    fn string(&mut self, max_len: u64) -> Result<String, UnpackError> {
        let len = self.integer()?;
        if len > max_len {
            return malformed(format!("a {len}-byte string where at most {max_len} are allowed"));
        }
        let mut bytes = vec![0_u8; len as usize];
        self.input.read_exact(&mut bytes).map_err(UnpackError::Read)?;
        self.skip_padding(len)?;
        String::from_utf8(bytes).or_else(|_| malformed("a string is not UTF-8"))
    }

    fn expect(&mut self, token: &str) -> Result<(), UnpackError> {
        let found = self.string(MAX_TOKEN_LEN)?;
        if found != token {
            return malformed(format!("expected {token:?}, found {found:?}"));
        }
        Ok(())
    }

    fn node(&mut self, path: &Path, depth: usize) -> Result<(), UnpackError> {
        if depth > MAX_DEPTH {
            return malformed(format!("directories nest deeper than {MAX_DEPTH}"));
        }
        self.expect("(")?;
        self.expect("type")?;
        match self.string(MAX_TOKEN_LEN)?.as_str() {
            "regular" => self.regular(path),
            "symlink" => self.symlink(path),
            "directory" => self.directory(path, depth),
            other => malformed(format!("unknown node type {other:?}")),
        }
    }

    fn regular(&mut self, path: &Path) -> Result<(), UnpackError> {
        let mut tag = self.string(MAX_TOKEN_LEN)?;
        let executable = tag == "executable";
        if executable {
            self.expect("")?;
            tag = self.string(MAX_TOKEN_LEN)?;
        }
        if tag != "contents" {
            return malformed(format!(r#"expected "contents", found {tag:?}"#));
        }
        let len = self.integer()?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(write_error(path))?;
        let copied =
            io::copy(&mut (&mut self.input).take(len), &mut file).map_err(write_error(path))?;
        if copied != len {
            return Err(UnpackError::Read(io::ErrorKind::UnexpectedEof.into()));
        }
        self.skip_padding(len)?;
        let mode = if executable { 0o555 } else { 0o444 };
        seal(&file, mode).map_err(write_error(path))?;
        self.expect(")")
    }

    fn symlink(&mut self, path: &Path) -> Result<(), UnpackError> {
        self.expect("target")?;
        let target = self.string(MAX_TARGET_LEN)?;
        symlink(target, path).map_err(write_error(path))?;
        self.expect(")")
    }

    fn directory(&mut self, path: &Path, depth: usize) -> Result<(), UnpackError> {
        fs::create_dir(path).map_err(write_error(path))?;
        let mut previous_name = None;
        loop {
            match self.string(MAX_TOKEN_LEN)?.as_str() {
                ")" => break,
                "entry" => self.entry(path, &mut previous_name, depth)?,
                other => return malformed(format!(r#"expected "entry", found {other:?}"#)),
            }
        }
        File::open(path)
            .and_then(|directory| seal(&directory, 0o555))
            .map_err(write_error(path))
    }

    fn entry(
        &mut self,
        directory: &Path,
        previous_name: &mut Option<String>,
        depth: usize,
    ) -> Result<(), UnpackError> {
        self.expect("(")?;
        self.expect("name")?;
        let name = self.string(MAX_NAME_LEN)?;
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0']) {
            return malformed(format!("{name:?} is not a valid entry name"));
        }
        if previous_name
            .as_ref()
            .is_some_and(|previous| *previous >= name)
        {
            return malformed(format!("entry {name:?} is out of order"));
        }
        self.expect("node")?;
        self.node(&directory.join(&name), depth + 1)?;
        *previous_name = Some(name);
        self.expect(")")
    }
}

/// Applies the store's permissions and timestamp to an open file or directory.
fn seal(file: &File, mode: u32) -> io::Result<()> {
    file.set_permissions(Permissions::from_mode(mode))?;
    let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(STORE_MTIME_SECONDS);
    file.set_times(FileTimes::new().set_modified(mtime))
}
