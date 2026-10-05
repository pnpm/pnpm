use std::fs;
use std::io;
use std::io::prelude::*;
use std::path::Path;
use std::str;

use crate::header::BLOCK_SIZE;
use crate::header::GNU_SPARSE_HEADERS_COUNT;
use crate::header::{path2bytes, HeaderMode};
use crate::GnuExtSparseHeader;
use crate::{other, EntryType, Header};

/// A structure for building archives
///
/// This structure has methods for building up an archive from scratch into any
/// arbitrary writer.
pub struct Builder<W: Write> {
    options: BuilderOptions,
    finished: bool,
    obj: Option<W>,
}

#[derive(Clone, Copy)]
struct BuilderOptions {
    mode: HeaderMode,
    preserve_absolute: bool,
    follow: bool,
    sparse: bool,
}

impl<W: Write> Builder<W> {
    /// Create a new archive builder with the underlying object as the
    /// destination of all data written. The builder will use
    /// `HeaderMode::Complete` by default.
    pub fn new(obj: W) -> Builder<W> {
        Builder {
            options: BuilderOptions {
                mode: HeaderMode::Complete,
                preserve_absolute: false,
                follow: true,
                sparse: true,
            },
            finished: false,
            obj: Some(obj),
        }
    }

    /// Changes the HeaderMode that will be used when reading fs Metadata for
    /// methods that implicitly read metadata for an input Path. Notably, this
    /// does _not_ apply to `append(Header)`.
    pub fn mode(&mut self, mode: HeaderMode) {
        self.options.mode = mode;
    }

    /// Peserve absolute path while creating an archive
    pub fn preserve_absolute(&mut self, preserve: bool) {
        self.options.preserve_absolute = preserve;
    }

    /// Control whether symlinks are followed when reading from the filesystem.
    /// Defaults to `true` (but see the note below — you almost certainly want
    /// to call `follow_symlinks(false)`).
    ///
    /// When `true`, symlinks are dereferenced: the archive entry contains the
    /// contents of the symlink target rather than the symlink itself,
    /// equivalent to GNU `tar --dereference` (`-h`). When `false` (the default
    /// for all mainstream tar implementations), symlinks are stored as symlink
    /// entries in the archive.
    ///
    /// # Why you should almost always use `follow_symlinks(false)`
    ///
    /// Every mainstream tar implementation preserves symlinks by default.
    /// GNU `tar` requires the explicit `--dereference` (`-h`) flag to follow
    /// them. Go's `archive/tar` stores whatever the underlying `fs.FS` reports
    /// and never dereferences on its own. BSD `tar` behaves the same way.
    /// This crate's default of `true` is a historical quirk kept for
    /// compatibility but is wrong for most use-cases:
    ///
    /// - Symlinks in the source tree are part of its structure and should
    ///   normally be preserved, not silently replaced by their targets.
    /// - When `true`, [`append_dir_all`](Builder::append_dir_all) follows
    ///   symlinks that point *outside* `src_path` just as readily as those
    ///   inside it. If the archiving process has broader filesystem read access
    ///   than whoever controls the source tree (e.g. a privileged backup
    ///   service, a CI runner archiving user-submitted workspaces), an attacker
    ///   can plant a symlink inside `src_path` to silently include arbitrary
    ///   files from the host.
    ///
    /// Call `follow_symlinks(false)` unless you have a specific reason to
    /// flatten symlinks into their targets. For the strongest guarantee, open
    /// `src_path` with [`cap-std`] and walk the tree with capability-safe I/O,
    /// which blocks symlink escapes at the OS level regardless of this setting.
    ///
    /// [`cap-std`]: https://docs.rs/cap-std/
    pub fn follow_symlinks(&mut self, follow: bool) {
        self.options.follow = follow;
    }

    /// Handle sparse files efficiently, if supported by the underlying
    /// filesystem. When true, sparse file information is read from disk and
    /// empty segments are omitted from the archive. Defaults to true.
    pub fn sparse(&mut self, sparse: bool) {
        self.options.sparse = sparse;
    }

    /// Gets shared reference to the underlying object.
    pub fn get_ref(&self) -> &W {
        self.obj.as_ref().unwrap()
    }

    /// Gets mutable reference to the underlying object.
    ///
    /// Note that care must be taken while writing to the underlying
    /// object. But, e.g. `get_mut().flush()` is claimed to be safe and
    /// useful in the situations when one needs to be ensured that
    /// tar entry was flushed to the disk.
    pub fn get_mut(&mut self) -> &mut W {
        self.obj.as_mut().unwrap()
    }

    /// Unwrap this archive, returning the underlying object.
    ///
    /// This function will finish writing the archive if the `finish` function
    /// hasn't yet been called, returning any I/O error which happens during
    /// that operation.
    pub fn into_inner(mut self) -> io::Result<W> {
        if !self.finished {
            self.finish()?;
        }
        Ok(self.obj.take().unwrap())
    }

    /// Adds a new entry to this archive.
    ///
    /// This function will append the header specified, followed by contents of
    /// the stream specified by `data`. To produce a valid archive the `size`
    /// field of `header` must be the same as the length of the stream that's
    /// being written. Additionally the checksum for the header should have been
    /// set via the `set_cksum` method.
    ///
    /// Note that this will not attempt to seek the archive to a valid position,
    /// so if the archive is in the middle of a read or some other similar
    /// operation then this may corrupt the archive.
    ///
    /// Also note that after all entries have been written to an archive the
    /// `finish` function needs to be called to finish writing the archive.
    ///
    /// # Errors
    ///
    /// This function will return an error for any intermittent I/O error which
    /// occurs when either reading or writing.
    ///
    /// # Examples
    ///
    /// ```
    /// use tar::{Builder, Header};
    ///
    /// let mut header = Header::new_gnu();
    /// header.set_path("foo").unwrap();
    /// header.set_size(4);
    /// header.set_cksum();
    ///
    /// let mut data: &[u8] = &[1, 2, 3, 4];
    ///
    /// let mut ar = Builder::new(Vec::new());
    /// ar.append(&header, data).unwrap();
    /// let data = ar.into_inner().unwrap();
    /// ```
    pub fn append<R: Read>(&mut self, header: &Header, mut data: R) -> io::Result<()> {
        append(self.get_mut(), header, &mut data)
    }

    /// Adds a new entry to this archive with the specified path.
    ///
    /// This function will set the specified path in the given header, which may
    /// require appending a GNU long-name extension entry to the archive first.
    /// The checksum for the header will be automatically updated via the
    /// `set_cksum` method after setting the path. No other metadata in the
    /// header will be modified.
    ///
    /// Then it will append the header, followed by contents of the stream
    /// specified by `data`. To produce a valid archive the `size` field of
    /// `header` must be the same as the length of the stream that's being
    /// written.
    ///
    /// Note that this will not attempt to seek the archive to a valid position,
    /// so if the archive is in the middle of a read or some other similar
    /// operation then this may corrupt the archive.
    ///
    /// Also note that after all entries have been written to an archive the
    /// `finish` function needs to be called to finish writing the archive.
    ///
    /// # Errors
    ///
    /// This function will return an error for any intermittent I/O error which
    /// occurs when either reading or writing.
    ///
    /// # Examples
    ///
    /// ```
    /// use tar::{Builder, Header};
    ///
    /// let mut header = Header::new_gnu();
    /// header.set_size(4);
    /// header.set_cksum();
    ///
    /// let mut data: &[u8] = &[1, 2, 3, 4];
    ///
    /// let mut ar = Builder::new(Vec::new());
    /// ar.append_data(&mut header, "really/long/path/to/foo", data).unwrap();
    /// let data = ar.into_inner().unwrap();
    /// ```
    pub fn append_data<P: AsRef<Path>, R: Read>(
        &mut self,
        header: &mut Header,
        path: P,
        data: R,
    ) -> io::Result<()> {
        let allow_absolute = self.options.preserve_absolute;
        prepare_header_path(self.get_mut(), header, path.as_ref(), allow_absolute)?;
        header.set_cksum();
        self.append(header, data)
    }

    /// Adds a new entry to this archive and returns an [`EntryWriter`] for
    /// adding its contents.
    ///
    /// This function is similar to [`Self::append_data`] but returns a
    /// [`io::Write`] implementation instead of taking data as a parameter.
    ///
    /// Similar constraints around the position of the archive and completion
    /// apply as with [`Self::append_data`]. It requires the underlying writer
    /// to implement [`Seek`] to update the header after writing the data.
    ///
    /// # Errors
    ///
    /// This function will return an error for any intermittent I/O error which
    /// occurs when either reading or writing.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io::Cursor;
    /// use std::io::Write as _;
    /// use tar::{Builder, Header};
    ///
    /// let mut header = Header::new_gnu();
    ///
    /// let mut ar = Builder::new(Cursor::new(Vec::new()));
    /// let mut entry = ar.append_writer(&mut header, "hi.txt").unwrap();
    /// entry.write_all(b"Hello, ").unwrap();
    /// entry.write_all(b"world!\n").unwrap();
    /// entry.finish().unwrap();
    /// ```
    pub fn append_writer<'a, P: AsRef<Path>>(
        &'a mut self,
        header: &'a mut Header,
        path: P,
    ) -> io::Result<EntryWriter<'a>>
    where
        W: Seek,
    {
        let allow_absolute = self.options.preserve_absolute;
        EntryWriter::start(self.get_mut(), header, path.as_ref(), allow_absolute)
    }

    /// Adds a new link (symbolic or hard) entry to this archive with the specified path and target.
    ///
    /// This function is similar to [`Self::append_data`] which supports long filenames,
    /// but also supports long link targets using GNU extensions if necessary.
    /// You must set the entry type to either [`EntryType::Link`] or [`EntryType::Symlink`].
    /// The `set_cksum` method will be invoked after setting the path. No other metadata in the
    /// header will be modified.
    ///
    /// If you are intending to use GNU extensions, you must use this method over calling
    /// [`Header::set_link_name`] because that function will fail on long links.
    ///
    /// Similar constraints around the position of the archive and completion
    /// apply as with [`Self::append_data`].
    ///
    /// # Errors
    ///
    /// This function will return an error for any intermittent I/O error which
    /// occurs when either reading or writing.
    ///
    /// # Examples
    ///
    /// ```
    /// use tar::{Builder, Header, EntryType};
    ///
    /// let mut ar = Builder::new(Vec::new());
    /// let mut header = Header::new_gnu();
    /// header.set_username("foo");
    /// header.set_entry_type(EntryType::Symlink);
    /// header.set_size(0);
    /// ar.append_link(&mut header, "really/long/path/to/foo", "other/really/long/target").unwrap();
    /// let data = ar.into_inner().unwrap();
    /// ```
    pub fn append_link<P: AsRef<Path>, T: AsRef<Path>>(
        &mut self,
        header: &mut Header,
        path: P,
        target: T,
    ) -> io::Result<()> {
        self._append_link(header, path.as_ref(), target.as_ref())
    }

    fn _append_link(&mut self, header: &mut Header, path: &Path, target: &Path) -> io::Result<()> {
        let allow_abolute = self.options.preserve_absolute;
        prepare_header_path(self.get_mut(), header, path, allow_abolute)?;
        prepare_header_link(self.get_mut(), header, target)?;
        header.set_cksum();
        self.append(header, std::io::empty())
    }

    /// Adds a file on the local filesystem to this archive.
    ///
    /// This function will open the file specified by `path` and insert the file
    /// into the archive with the appropriate metadata set, returning any I/O
    /// error which occurs while writing. The path name for the file inside of
    /// this archive will be the same as `path`, and it is required that the
    /// path is a relative path.
    ///
    /// Note that this will not attempt to seek the archive to a valid position,
    /// so if the archive is in the middle of a read or some other similar
    /// operation then this may corrupt the archive.
    ///
    /// Also note that after all files have been written to an archive the
    /// `finish` function needs to be called to finish writing the archive.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tar::Builder;
    ///
    /// let mut ar = Builder::new(Vec::new());
    ///
    /// ar.append_path("foo/bar.txt").unwrap();
    /// ```
    pub fn append_path<P: AsRef<Path>>(&mut self, path: P) -> io::Result<()> {
        let options = self.options;
        append_path_with_name(self.get_mut(), path.as_ref(), None, options)
    }

    /// Adds a file on the local filesystem to this archive under another name.
    ///
    /// This function will open the file specified by `path` and insert the file
    /// into the archive as `name` with appropriate metadata set, returning any
    /// I/O error which occurs while writing. The path name for the file inside
    /// of this archive will be `name` is required to be a relative path.
    ///
    /// Note that this will not attempt to seek the archive to a valid position,
    /// so if the archive is in the middle of a read or some other similar
    /// operation then this may corrupt the archive.
    ///
    /// Note if the `path` is a directory. This will just add an entry to the archive,
    /// rather than contents of the directory.
    ///
    /// Also note that after all files have been written to an archive the
    /// `finish` function needs to be called to finish writing the archive.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use tar::Builder;
    ///
    /// let mut ar = Builder::new(Vec::new());
    ///
    /// // Insert the local file "foo/bar.txt" in the archive but with the name
    /// // "bar/foo.txt".
    /// ar.append_path_with_name("foo/bar.txt", "bar/foo.txt").unwrap();
    /// ```
    pub fn append_path_with_name<P: AsRef<Path>, N: AsRef<Path>>(
        &mut self,
        path: P,
        name: N,
    ) -> io::Result<()> {
        let options = self.options;
        append_path_with_name(self.get_mut(), path.as_ref(), Some(name.as_ref()), options)
    }

    /// Adds a file to this archive with the given path as the name of the file
    /// in the archive.
    ///
    /// This will use the metadata of `file` to populate a `Header`, and it will
    /// then append the file to the archive with the name `path`.
    ///
    /// Note that this will not attempt to seek the archive to a valid position,
    /// so if the archive is in the middle of a read or some other similar
    /// operation then this may corrupt the archive.
    ///
    /// Also note that after all files have been written to an archive the
    /// `finish` function needs to be called to finish writing the archive.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs::File;
    /// use tar::Builder;
    ///
    /// let mut ar = Builder::new(Vec::new());
    ///
    /// // Open the file at one location, but insert it into the archive with a
    /// // different name.
    /// let mut f = File::open("foo/bar/baz.txt").unwrap();
    /// ar.append_file("bar/baz.txt", &mut f).unwrap();
    /// ```
    pub fn append_file<P: AsRef<Path>>(&mut self, path: P, file: &mut fs::File) -> io::Result<()> {
        let options = self.options;
        append_file(self.get_mut(), path.as_ref(), file, options)
    }

    /// Adds a directory to this archive with the given path as the name of the
    /// directory in the archive.
    ///
    /// This will use `stat` to populate a `Header`, and it will then append the
    /// directory to the archive with the name `path`.
    ///
    /// Note that this will not attempt to seek the archive to a valid position,
    /// so if the archive is in the middle of a read or some other similar
    /// operation then this may corrupt the archive.
    ///
    /// Note this will not add the contents of the directory to the archive.
    /// See `append_dir_all` for recursively adding the contents of the directory.
    ///
    /// Also note that after all files have been written to an archive the
    /// `finish` function needs to be called to finish writing the archive.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::fs;
    /// use tar::Builder;
    ///
    /// let mut ar = Builder::new(Vec::new());
    ///
    /// // Use the directory at one location, but insert it into the archive
    /// // with a different name.
    /// ar.append_dir("bardir", ".").unwrap();
    /// ```
    pub fn append_dir<P, Q>(&mut self, path: P, src_path: Q) -> io::Result<()>
    where
        P: AsRef<Path>,
        Q: AsRef<Path>,
    {
        let options = self.options;
        append_dir(self.get_mut(), path.as_ref(), src_path.as_ref(), options)
    }

    /// Adds a directory and all of its contents (recursively) to this archive
    /// with the given path as the name of the directory in the archive.
    ///
    /// Note that this will not attempt to seek the archive to a valid position,
    /// so if the archive is in the middle of a read or some other similar
    /// operation then this may corrupt the archive.
    ///
    /// Also note that after all files have been written to an archive the
    /// `finish` or `into_inner` function needs to be called to finish
    /// writing the archive.
    ///
    /// # Security
    ///
    /// **Call [`follow_symlinks(false)`](Builder::follow_symlinks) before this
    /// method** unless you have an explicit reason to dereference symlinks.
    /// All mainstream tar implementations (GNU tar, BSD tar, Go's
    /// `archive/tar`) preserve symlinks by default; this crate's default of
    /// `true` is a historical quirk.
    ///
    /// When `follow_symlinks` is `true` (the current default), this method
    /// dereferences every symlink it encounters, including ones whose targets
    /// lie **outside** `src_path`. When the archiver runs with broader
    /// filesystem access than whoever controls the source tree (e.g. a
    /// privileged backup or export service), an attacker can plant a symlink
    /// inside `src_path` to silently include arbitrary files the archiver can
    /// read, with no indication in the archive that they came from outside the
    /// source root.
    ///
    /// ```no_run
    /// use tar::Builder;
    ///
    /// # let src_path = std::path::Path::new(".");
    /// # let writer = std::io::sink();
    /// // Recommended: preserve symlinks as-is, matching GNU tar's default.
    /// let mut ar = Builder::new(writer);
    /// ar.follow_symlinks(false);
    /// ar.append_dir_all("", src_path).unwrap();
    /// ar.finish().unwrap();
    /// ```
    ///
    /// With `follow_symlinks(false)`, symlinks inside the source tree are
    /// stored as symlink entries in the archive rather than being read through.
    /// Note that the resulting archive may then contain symlinks with absolute
    /// or `..`-relative targets; validate or strip those on extraction if the
    /// archive consumer is also untrusted.
    ///
    /// For the strongest available guarantee, open `src_path` using [`cap-std`]
    /// and walk the directory tree with capability-safe I/O. This prevents
    /// symlink escapes at the OS level and protects against TOCTOU races that
    /// a purely path-based check cannot close.
    ///
    /// [`cap-std`]: https://docs.rs/cap-std/
    ///
    /// # Examples
    ///
    /// ```
    /// use std::fs;
    /// use tar::Builder;
    ///
    /// let mut ar = Builder::new(Vec::new());
    ///
    /// // Use the directory at one location ("."), but insert it into the archive
    /// // with a different name ("bardir").
    /// ar.append_dir_all("bardir", ".").unwrap();
    /// ar.finish().unwrap();
    /// ```
    ///
    /// Use `append_dir_all` with an empty string as the first path argument to
    /// create an archive from all files in a directory without renaming.
    ///
    /// ```
    /// use std::fs;
    /// use std::path::PathBuf;
    /// use tar::{Archive, Builder};
    ///
    /// let tmpdir = tempfile::tempdir().unwrap();
    /// let path = tmpdir.path();
    /// fs::write(path.join("a.txt"), b"hello").unwrap();
    /// fs::write(path.join("b.txt"), b"world").unwrap();
    ///
    /// // Create a tarball from the files in the directory
    /// let mut ar = Builder::new(Vec::new());
    /// ar.append_dir_all("", path).unwrap();
    ///
    /// // List files in the archive
    /// let archive = ar.into_inner().unwrap();
    /// let archived_files = Archive::new(archive.as_slice())
    ///     .entries()
    ///     .unwrap()
    ///     .map(|entry| entry.unwrap().path().unwrap().into_owned())
    ///     .collect::<Vec<_>>();
    ///
    /// assert!(archived_files.contains(&PathBuf::from("a.txt")));
    /// assert!(archived_files.contains(&PathBuf::from("b.txt")));
    /// ```
    pub fn append_dir_all<P, Q>(&mut self, path: P, src_path: Q) -> io::Result<()>
    where
        P: AsRef<Path>,
        Q: AsRef<Path>,
    {
        let options = self.options;
        append_dir_all(self.get_mut(), path.as_ref(), src_path.as_ref(), options)
    }

    /// Finish writing this archive, emitting the termination sections.
    ///
    /// This function should only be called when the archive has been written
    /// entirely and if an I/O error happens the underlying object still needs
    /// to be acquired.
    ///
    /// In most situations the `into_inner` method should be preferred.
    pub fn finish(&mut self) -> io::Result<()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        self.get_mut().write_all(&[0; 1024])
    }
}

trait SeekWrite: Write + Seek {
    fn as_write(&mut self) -> &mut dyn Write;
}

impl<T: Write + Seek> SeekWrite for T {
    fn as_write(&mut self) -> &mut dyn Write {
        self
    }
}

/// A writer for a single entry in a tar archive.
///
/// This struct is returned by [`Builder::append_writer`] and provides a
/// [`Write`] implementation for adding content to an archive entry.
///
/// After writing all data to the entry, it must be finalized either by
/// explicitly calling [`EntryWriter::finish`] or by letting it drop.
pub struct EntryWriter<'a> {
    // NOTE: Do not add any fields here which require Drop!
    // See the comment below in finish().
    obj: &'a mut dyn SeekWrite,
    header: &'a mut Header,
    written: u64,
}

impl EntryWriter<'_> {
    fn start<'a>(
        obj: &'a mut dyn SeekWrite,
        header: &'a mut Header,
        path: &Path,
        allow_absolute: bool,
    ) -> io::Result<EntryWriter<'a>> {
        prepare_header_path(obj.as_write(), header, path, allow_absolute)?;

        // Reserve space for header, will be overwritten once data is written.
        obj.write_all([0u8; BLOCK_SIZE as usize].as_ref())?;

        Ok(EntryWriter {
            obj,
            header,
            written: 0,
        })
    }

    /// Finish writing the current entry in the archive.
    pub fn finish(self) -> io::Result<()> {
        // NOTE: This is an optimization for "fallible destructuring".
        // We want finish() to return an error, but we also need to invoke
        // cleanup in our Drop handler, which will run unconditionally
        // and try to do the same work.
        // By using ManuallyDrop, we suppress that drop. However, this would
        // be a memory leak if we ever had any struct members which required
        // Drop - which we don't right now.
        // But if we ever gain one, we will need to change to use e.g. Option<>
        // around some of the fields or have a `bool finished` etc.
        let mut this = std::mem::ManuallyDrop::new(self);
        this.do_finish()
    }

    fn do_finish(&mut self) -> io::Result<()> {
        // Pad with zeros if necessary.
        let buf = [0u8; BLOCK_SIZE as usize];
        let remaining = BLOCK_SIZE.wrapping_sub(self.written) % BLOCK_SIZE;
        self.obj.write_all(&buf[..remaining as usize])?;
        let written = (self.written + remaining) as i64;

        // Seek back to the header position.
        self.obj
            .seek(io::SeekFrom::Current(-written - BLOCK_SIZE as i64))?;

        self.header.set_size(self.written);
        self.header.set_cksum();
        self.obj.write_all(self.header.as_bytes())?;

        // Seek forward to restore the position.
        self.obj.seek(io::SeekFrom::Current(written))?;

        Ok(())
    }
}

impl Write for EntryWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = self.obj.write(buf)?;
        self.written += len as u64;
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.obj.flush()
    }
}

impl Drop for EntryWriter<'_> {
    fn drop(&mut self) {
        let _ = self.do_finish();
    }
}

fn append(mut dst: &mut dyn Write, header: &Header, mut data: &mut dyn Read) -> io::Result<()> {
    dst.write_all(header.as_bytes())?;
    let len = io::copy(&mut data, &mut dst)?;
    pad_zeroes(&mut dst, len)?;
    Ok(())
}

fn pad_zeroes(dst: &mut dyn Write, len: u64) -> io::Result<()> {
    let buf = [0; BLOCK_SIZE as usize];
    let remaining = BLOCK_SIZE - (len % BLOCK_SIZE);
    if remaining < BLOCK_SIZE {
        dst.write_all(&buf[..remaining as usize])?;
    }
    Ok(())
}

fn append_path_with_name(
    dst: &mut dyn Write,
    path: &Path,
    name: Option<&Path>,
    options: BuilderOptions,
) -> io::Result<()> {
    let stat = if options.follow {
        fs::metadata(path).map_err(|err| {
            io::Error::new(
                err.kind(),
                format!("{} when getting metadata for {}", err, path.display()),
            )
        })?
    } else {
        fs::symlink_metadata(path).map_err(|err| {
            io::Error::new(
                err.kind(),
                format!("{} when getting metadata for {}", err, path.display()),
            )
        })?
    };
    let ar_name = name.unwrap_or(path);
    if stat.is_file() {
        append_file(dst, ar_name, &mut fs::File::open(path)?, options)
    } else if stat.is_dir() {
        append_fs(
            dst,
            ar_name,
            &stat,
            options.mode,
            options.preserve_absolute,
            None,
        )
    } else if stat.file_type().is_symlink() {
        let link_name = fs::read_link(path)?;
        append_fs(
            dst,
            ar_name,
            &stat,
            options.mode,
            options.preserve_absolute,
            Some(&link_name),
        )
    } else {
        #[cfg(unix)]
        {
            append_special(dst, path, &stat, options.mode, options.preserve_absolute)
        }
        #[cfg(not(unix))]
        {
            Err(other(&format!("{} has unknown file type", path.display())))
        }
    }
}

#[cfg(unix)]
fn append_special(
    dst: &mut dyn Write,
    path: &Path,
    stat: &fs::Metadata,
    mode: HeaderMode,
    allow_absolute: bool,
) -> io::Result<()> {
    use ::std::os::unix::fs::{FileTypeExt, MetadataExt};

    let file_type = stat.file_type();
    let entry_type;
    if file_type.is_socket() {
        // sockets can't be archived
        return Err(other(&format!(
            "{}: socket can not be archived",
            path.display()
        )));
    } else if file_type.is_fifo() {
        entry_type = EntryType::Fifo;
    } else if file_type.is_char_device() {
        entry_type = EntryType::Char;
    } else if file_type.is_block_device() {
        entry_type = EntryType::Block;
    } else {
        return Err(other(&format!("{} has unknown file type", path.display())));
    }

    let mut header = Header::new_gnu();
    header.set_metadata_in_mode(stat, mode);
    prepare_header_path(dst, &mut header, path, allow_absolute)?;

    header.set_entry_type(entry_type);
    let dev_id = stat.rdev();
    let dev_major = ((dev_id >> 32) & 0xffff_f000) | ((dev_id >> 8) & 0x0000_0fff);
    let dev_minor = ((dev_id >> 12) & 0xffff_ff00) | ((dev_id) & 0x0000_00ff);
    header.set_device_major(dev_major as u32)?;
    header.set_device_minor(dev_minor as u32)?;

    header.set_cksum();
    dst.write_all(header.as_bytes())?;

    Ok(())
}

fn append_file(
    dst: &mut dyn Write,
    path: &Path,
    file: &mut fs::File,
    options: BuilderOptions,
) -> io::Result<()> {
    let stat = file.metadata()?;
    let mut header = Header::new_gnu();

    prepare_header_path(dst, &mut header, path, options.preserve_absolute)?;
    header.set_metadata_in_mode(&stat, options.mode);
    let sparse_entries = if options.sparse {
        prepare_header_sparse(file, &stat, &mut header)?
    } else {
        None
    };
    header.set_cksum();
    dst.write_all(header.as_bytes())?;

    if let Some(sparse_entries) = sparse_entries {
        append_extended_sparse_headers(dst, &sparse_entries)?;
        for entry in sparse_entries.entries {
            file.seek(io::SeekFrom::Start(entry.offset))?;
            io::copy(&mut file.take(entry.num_bytes), dst)?;
        }
        pad_zeroes(dst, sparse_entries.on_disk_size)?;
    } else {
        let len = io::copy(file, dst)?;
        pad_zeroes(dst, len)?;
    }

    Ok(())
}

fn append_dir(
    dst: &mut dyn Write,
    path: &Path,
    src_path: &Path,
    options: BuilderOptions,
) -> io::Result<()> {
    let stat = fs::metadata(src_path)?;
    append_fs(
        dst,
        path,
        &stat,
        options.mode,
        options.preserve_absolute,
        None,
    )
}

fn prepare_header(size: u64, entry_type: u8) -> Header {
    let mut header = Header::new_gnu();
    let name = b"././@LongLink";
    header.as_gnu_mut().unwrap().name[..name.len()].clone_from_slice(&name[..]);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    // + 1 to be compliant with GNU tar
    header.set_size(size + 1);
    header.set_entry_type(EntryType::new(entry_type));
    header.set_cksum();
    header
}

fn prepare_header_path(
    dst: &mut dyn Write,
    header: &mut Header,
    path: &Path,
    allow_absolute: bool,
) -> io::Result<()> {
    // Try to encode the path directly in the header, but if it ends up not
    // working (probably because it's too long) then try to use the GNU-specific
    // long name extension by emitting an entry which indicates that it's the
    // filename.
    let result = if allow_absolute {
        header.set_path_absolute(path)
    } else {
        header.set_path(path)
    };

    if let Err(e) = result {
        let data = path2bytes(path)?;
        let max = header.as_old().name.len();
        // Since `e` isn't specific enough to let us know the path is indeed too
        // long, verify it first before using the extension.
        if data.len() < max {
            return Err(e);
        }
        // Truncate the path to store in the header we're about to emit to
        // ensure we've got something at least mentioned. Note that we use
        // `str`-encoding to be compatible with Windows, but in general the
        // entry in the header itself shouldn't matter too much since extraction
        // doesn't look at it.
        //
        // Validate the truncated path BEFORE writing the long-name extension
        // to the stream. If validation fails after writing, the orphaned
        // extension entry corrupts subsequent archive entries.
        let truncated = match str::from_utf8(&data[..max]) {
            Ok(s) => s,
            Err(e) => str::from_utf8(&data[..e.valid_up_to()]).unwrap(),
        };
        header.set_truncated_path_for_gnu_header(truncated, allow_absolute)?;

        let header2 = prepare_header(data.len() as u64, b'L');
        // null-terminated string
        let mut data2 = data.chain(io::repeat(0).take(1));
        append(dst, &header2, &mut data2)?;
    }
    Ok(())
}

fn prepare_header_link(
    dst: &mut dyn Write,
    header: &mut Header,
    link_name: &Path,
) -> io::Result<()> {
    // Same as previous function but for linkname
    if let Err(e) = header.set_link_name(link_name) {
        let data = path2bytes(link_name)?;
        if data.len() < header.as_old().linkname.len() {
            return Err(e);
        }
        let header2 = prepare_header(data.len() as u64, b'K');
        let mut data2 = data.chain(io::repeat(0).take(1));
        append(dst, &header2, &mut data2)?;
    }
    Ok(())
}

fn prepare_header_sparse(
    file: &mut fs::File,
    stat: &fs::Metadata,
    header: &mut Header,
) -> io::Result<Option<SparseEntries>> {
    let entries = match find_sparse_entries(file, stat)? {
        Some(entries) => entries,
        _ => return Ok(None),
    };

    header.set_entry_type(EntryType::GNUSparse);
    header.set_size(entries.on_disk_size);

    // Write the first 4 (GNU_SPARSE_HEADERS_COUNT) entries to the given header.
    // The remaining entries will be written as subsequent extended headers. See
    // https://www.gnu.org/software/tar/manual/html_section/Sparse-Formats.html#Old-GNU-Format
    // for details on the format.
    let gnu_header = &mut header.as_gnu_mut().unwrap();
    gnu_header.set_real_size(entries.size());

    for (entry, header_entry) in std::iter::zip(&entries.entries, &mut gnu_header.sparse) {
        header_entry.set_offset(entry.offset);
        header_entry.set_length(entry.num_bytes);
    }
    gnu_header.set_is_extended(entries.entries.len() > gnu_header.sparse.len());

    Ok(Some(entries))
}

/// Write extra sparse headers into `dst` for those entries that did not fit in the main header.
fn append_extended_sparse_headers(dst: &mut dyn Write, entries: &SparseEntries) -> io::Result<()> {
    // The first `GNU_SPARSE_HEADERS_COUNT` entries are written to the main header, so skip them.
    let mut it = entries
        .entries
        .iter()
        .skip(GNU_SPARSE_HEADERS_COUNT)
        .peekable();

    // Each GnuExtSparseHeader can hold up to fixed number of sparse entries (21).
    // So we pack entries into multiple headers if necessary.
    while it.peek().is_some() {
        let mut ext_header = GnuExtSparseHeader::new();
        for header_entry in ext_header.sparse.iter_mut() {
            if let Some(entry) = it.next() {
                header_entry.set_offset(entry.offset);
                header_entry.set_length(entry.num_bytes);
            } else {
                break;
            }
        }
        ext_header.set_is_extended(it.peek().is_some());
        dst.write_all(ext_header.as_bytes())?;
    }

    Ok(())
}

fn append_fs(
    dst: &mut dyn Write,
    path: &Path,
    meta: &fs::Metadata,
    mode: HeaderMode,
    allow_absolute: bool,
    link_name: Option<&Path>,
) -> io::Result<()> {
    let mut header = Header::new_gnu();

    prepare_header_path(dst, &mut header, path, allow_absolute)?;
    header.set_metadata_in_mode(meta, mode);
    if let Some(link_name) = link_name {
        prepare_header_link(dst, &mut header, link_name)?;
    }
    header.set_cksum();
    dst.write_all(header.as_bytes())
}

fn append_dir_all(
    dst: &mut dyn Write,
    path: &Path,
    src_path: &Path,
    options: BuilderOptions,
) -> io::Result<()> {
    let mut stack = vec![(src_path.to_path_buf(), true, false)];
    while let Some((src, is_dir, is_symlink)) = stack.pop() {
        let dest = path.join(src.strip_prefix(src_path).unwrap());
        // In case of a symlink pointing to a directory, is_dir is false, but src.is_dir() will return true
        if is_dir || (is_symlink && options.follow && src.is_dir()) {
            for entry in fs::read_dir(&src)? {
                let entry = entry?;
                let file_type = entry.file_type()?;
                stack.push((entry.path(), file_type.is_dir(), file_type.is_symlink()));
            }
            if dest != Path::new("") {
                append_dir(dst, &dest, &src, options)?;
            }
        } else if !options.follow && is_symlink {
            let stat = fs::symlink_metadata(&src)?;
            let link_name = fs::read_link(&src)?;
            append_fs(
                dst,
                &dest,
                &stat,
                options.mode,
                options.preserve_absolute,
                Some(&link_name),
            )?;
        } else {
            #[cfg(unix)]
            {
                let stat = fs::metadata(&src)?;
                if !stat.is_file() {
                    append_special(dst, &dest, &stat, options.mode, options.preserve_absolute)?;
                    continue;
                }
            }
            append_file(dst, &dest, &mut fs::File::open(src)?, options)?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SparseEntries {
    entries: Vec<SparseEntry>,
    on_disk_size: u64,
}

impl SparseEntries {
    fn size(&self) -> u64 {
        self.entries.last().map_or(0, |e| e.offset + e.num_bytes)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct SparseEntry {
    offset: u64,
    num_bytes: u64,
}

/// Find sparse entries in a file. Returns:
/// * `Ok(Some(_))` if the file is sparse.
/// * `Ok(None)` if the file is not sparse, or if the file system does not support sparse files.
/// * `Err(_)` if an error occurred. The lack of support for sparse files is not
///   considered an error. It might return an error if the file is modified
///   while reading.
fn find_sparse_entries(
    file: &mut fs::File,
    stat: &fs::Metadata,
) -> io::Result<Option<SparseEntries>> {
    #[cfg(not(any(target_os = "android", target_os = "freebsd", target_os = "linux")))]
    {
        let _ = file;
        let _ = stat;
        Ok(None)
    }

    #[cfg(any(target_os = "android", target_os = "freebsd", target_os = "linux"))]
    find_sparse_entries_seek(file, stat)
}

/// Implementation of `find_sparse_entries` using `SEEK_HOLE` and `SEEK_DATA`.
#[cfg(any(target_os = "android", target_os = "freebsd", target_os = "linux"))]
fn find_sparse_entries_seek(
    file: &mut fs::File,
    stat: &fs::Metadata,
) -> io::Result<Option<SparseEntries>> {
    use std::os::unix::fs::MetadataExt as _;
    use std::os::unix::io::AsRawFd as _;

    fn lseek(file: &fs::File, offset: i64, whence: libc::c_int) -> Result<i64, i32> {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let lseek = libc::lseek64;
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        let lseek = libc::lseek;

        match unsafe { lseek(file.as_raw_fd(), offset, whence) } {
            -1 => Err(io::Error::last_os_error().raw_os_error().unwrap()),
            off => Ok(off),
        }
    }

    if stat.blocks() == 0 {
        return Ok(if stat.size() == 0 {
            // Empty file.
            None
        } else {
            // Fully sparse file.
            Some(SparseEntries {
                entries: vec![SparseEntry {
                    offset: stat.size(),
                    num_bytes: 0,
                }],
                on_disk_size: 0,
            })
        });
    }

    // On most Unixes, we need to read `_PC_MIN_HOLE_SIZE` to see if the file
    // system supports `SEEK_HOLE`.
    // FreeBSD: https://man.freebsd.org/cgi/man.cgi?query=lseek&sektion=2&manpath=FreeBSD+14.1-STABLE
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    if unsafe { libc::fpathconf(file.as_raw_fd(), libc::_PC_MIN_HOLE_SIZE) } == -1 {
        return Ok(None);
    }

    // Linux is the only UNIX-like without support for `_PC_MIN_HOLE_SIZE`, so
    // instead we try to call `lseek` and see if it fails.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    match lseek(file, 0, libc::SEEK_HOLE) {
        Ok(_) => (),
        Err(libc::ENXIO) => {
            // The file is empty. Treat it as non-sparse.
            return Ok(None);
        }
        Err(_) => return Ok(None),
    }

    let mut entries = Vec::new();
    let mut on_disk_size = 0;
    let mut off_s = 0;
    loop {
        //  off_s=0      │     off_s               │ off_s
        //    ↓          │       ↓                 │   ↓
        //    | DATA |…  │  ……………| HOLE | DATA |…  │  …|×EOF×
        //    ↑          │       ↑      ↑          │
        //   (a)         │  (b) (c)    (d)         │     (e)
        match lseek(file, off_s, libc::SEEK_DATA) {
            Ok(0) if off_s == 0 => (), // (a) The file starts with data.
            Ok(off) if off < off_s => {
                // (b) Unlikely.
                return Err(std::io::Error::new(
                    io::ErrorKind::Other,
                    "lseek(SEEK_DATA) went backwards",
                ));
            }
            Ok(off) if off == off_s => {
                // (c) The data at the same offset as the hole.
                return Err(std::io::Error::new(
                    io::ErrorKind::Other,
                    "lseek(SEEK_DATA) did not advance. \
                     Did the file change while appending?",
                ));
            }
            Ok(off) => off_s = off,    // (d) Jump to the next hole.
            Err(libc::ENXIO) => break, // (e) Reached the end of the file.
            Err(errno) => return Err(io::Error::from_raw_os_error(errno)),
        };

        // off_s=0          │     off_s               │    off_s
        //   ↓              │       ↓                 │      ↓
        //   | DATA |×EOF×  │  ……………| DATA | HOLE |…  │  …|×EOF×
        //          ↑       │       ↑      ↑          │
        //         (a)      │  (b) (c)    (d)         │     (e)
        match lseek(file, off_s, libc::SEEK_HOLE) {
            Ok(off_e) if off_s == 0 && (off_e as u64) == stat.size() => {
                // (a) The file is not sparse.
                file.seek(io::SeekFrom::Start(0))?;
                return Ok(None);
            }
            Ok(off_e) if off_e < off_s => {
                // (b) Unlikely.
                return Err(std::io::Error::new(
                    io::ErrorKind::Other,
                    "lseek(SEEK_HOLE) went backwards",
                ));
            }
            Ok(off_e) if off_e == off_s => {
                // (c) The hole at the same offset as the data.
                return Err(std::io::Error::new(
                    io::ErrorKind::Other,
                    "lseek(SEEK_HOLE) did not advance. \
                     Did the file change while appending?",
                ));
            }
            Ok(off_e) => {
                // (d) Found a hole or reached the end of the file (implicit
                // zero-length hole).
                entries.push(SparseEntry {
                    offset: off_s as u64,
                    num_bytes: off_e as u64 - off_s as u64,
                });
                on_disk_size += off_e as u64 - off_s as u64;
                off_s = off_e;
            }
            Err(libc::ENXIO) => {
                // (e) off_s was already beyond the end of the file.
                return Err(std::io::Error::new(
                    io::ErrorKind::Other,
                    "lseek(SEEK_HOLE) returned ENXIO. \
                     Did the file change while appending?",
                ));
            }
            Err(errno) => return Err(io::Error::from_raw_os_error(errno)),
        };
    }

    if off_s as u64 > stat.size() {
        return Err(std::io::Error::new(
            io::ErrorKind::Other,
            "lseek(SEEK_DATA) went beyond the end of the file. \
             Did the file change while appending?",
        ));
    }

    // Add a final zero-length entry. It is required if the file ends with a
    // hole, and redundant otherwise. However, we add it unconditionally to
    // mimic GNU tar behavior.
    entries.push(SparseEntry {
        offset: stat.size(),
        num_bytes: 0,
    });

    file.seek(io::SeekFrom::Start(0))?;

    Ok(Some(SparseEntries {
        entries,
        on_disk_size,
    }))
}

impl<W: Write> Drop for Builder<W> {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Should be multiple of 4KiB on ext4, multiple of 32KiB on FreeBSD/UFS, multiple of 64KiB on
    /// ppc64el
    const SPARSE_BLOCK_SIZE: u64 = 64 * 1024;

    #[test]
    fn test_find_sparse_entries() {
        let cases: &[(&str, &[SparseEntry])] = &[
            ("|", &[]),
            (
                "|    |    |    |    |",
                &[SparseEntry {
                    offset: 4 * SPARSE_BLOCK_SIZE,
                    num_bytes: 0,
                }],
            ),
            (
                "|####|####|####|####|",
                &[
                    SparseEntry {
                        offset: 0,
                        num_bytes: 4 * SPARSE_BLOCK_SIZE,
                    },
                    SparseEntry {
                        offset: 4 * SPARSE_BLOCK_SIZE,
                        num_bytes: 0,
                    },
                ],
            ),
            (
                "|####|####|    |    |",
                &[
                    SparseEntry {
                        offset: 0,
                        num_bytes: 2 * SPARSE_BLOCK_SIZE,
                    },
                    SparseEntry {
                        offset: 4 * SPARSE_BLOCK_SIZE,
                        num_bytes: 0,
                    },
                ],
            ),
            (
                "|    |    |####|####|",
                &[
                    SparseEntry {
                        offset: 2 * SPARSE_BLOCK_SIZE,
                        num_bytes: 2 * SPARSE_BLOCK_SIZE,
                    },
                    SparseEntry {
                        offset: 4 * SPARSE_BLOCK_SIZE,
                        num_bytes: 0,
                    },
                ],
            ),
            (
                "|####|    |####|    |",
                &[
                    SparseEntry {
                        offset: 0,
                        num_bytes: SPARSE_BLOCK_SIZE,
                    },
                    SparseEntry {
                        offset: 2 * SPARSE_BLOCK_SIZE,
                        num_bytes: SPARSE_BLOCK_SIZE,
                    },
                    SparseEntry {
                        offset: 4 * SPARSE_BLOCK_SIZE,
                        num_bytes: 0,
                    },
                ],
            ),
            (
                "|####|    |    |####|",
                &[
                    SparseEntry {
                        offset: 0,
                        num_bytes: SPARSE_BLOCK_SIZE,
                    },
                    SparseEntry {
                        offset: 3 * SPARSE_BLOCK_SIZE,
                        num_bytes: SPARSE_BLOCK_SIZE,
                    },
                    SparseEntry {
                        offset: 4 * SPARSE_BLOCK_SIZE,
                        num_bytes: 0,
                    },
                ],
            ),
            (
                "|    |####|####|    |",
                &[
                    SparseEntry {
                        offset: SPARSE_BLOCK_SIZE,
                        num_bytes: 2 * SPARSE_BLOCK_SIZE,
                    },
                    SparseEntry {
                        offset: 4 * SPARSE_BLOCK_SIZE,
                        num_bytes: 0,
                    },
                ],
            ),
        ];

        let mut file = tempfile::tempfile().unwrap();

        for &(description, map) in cases {
            file.set_len(0).unwrap();
            file.set_len(map.last().map_or(0, |e| e.offset + e.num_bytes))
                .unwrap();

            for e in map {
                file.seek(io::SeekFrom::Start(e.offset)).unwrap();
                for _ in 0..e.num_bytes / SPARSE_BLOCK_SIZE {
                    file.write_all(&[0xFF; SPARSE_BLOCK_SIZE as usize]).unwrap();
                }
            }

            let expected = match map {
                // Empty file.
                &[] => None,

                // 100% dense.
                &[SparseEntry {
                    offset: 0,
                    num_bytes: x1,
                }, SparseEntry {
                    offset: x2,
                    num_bytes: 0,
                }] if x1 == x2 => None,

                // Sparse.
                map => Some(SparseEntries {
                    entries: map.to_vec(),
                    on_disk_size: map.iter().map(|e| e.num_bytes).sum(),
                }),
            };

            let stat = file.metadata().unwrap();
            let reported = find_sparse_entries(&mut file, &stat).unwrap();

            // Loose check: we did not miss any data blocks.
            if let Err(e) = loose_check_sparse_entries(reported.as_ref(), expected.as_ref()) {
                panic!(
                    "Case: {description}\n\
                     Reported: {reported:?}\n\
                     Expected: {expected:?}\n\
                     Error: {e}",
                );
            }

            // On Linux, always do a strict check. Skip on FreeBSD, as on UFS
            // the last block is always dense, even if it's zero-filled.
            #[cfg(any(target_os = "android", target_os = "linux"))]
            assert_eq!(reported, expected, "Case: {description}");
        }
    }

    fn loose_check_sparse_entries(
        reported: Option<&SparseEntries>,
        expected: Option<&SparseEntries>,
    ) -> Result<(), &'static str> {
        let reported = match reported {
            Some(entries) => entries, // Reported as sparse.
            // It's not an error to report a sparse file as non-sparse.
            None => return Ok(()),
        };
        let expected = match expected {
            Some(entries) => entries,
            None => return Err("Expected dense file, but reported as sparse"),
        };

        // Check that we didn't miss any data blocks. However, reporting some
        // holes as data is not an error during the loose check.
        if expected.entries.iter().any(|e| {
            !reported
                .entries
                .iter()
                .any(|r| e.offset >= r.offset && e.offset + e.num_bytes <= r.offset + r.num_bytes)
        }) {
            return Err("Reported is not a superset of expected");
        }

        if reported.entries.last() != expected.entries.last() {
            return Err("Last zero-length entry is not as expected");
        }

        // Check invariants of SparseEntries.
        let mut prev_end = None;
        for e in &reported.entries[..reported.entries.len()] {
            if prev_end.map_or(false, |p| e.offset < p) {
                return Err("Overlapping or unsorted entries");
            }
            prev_end = Some(e.offset + e.num_bytes);
        }

        if reported.on_disk_size != reported.entries.iter().map(|e| e.num_bytes).sum() {
            return Err("Incorrect on-disk size");
        }

        Ok(())
    }
}
