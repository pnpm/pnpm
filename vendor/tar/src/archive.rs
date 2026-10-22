use std::cell::{Cell, RefCell};
use std::cmp;
use std::convert::TryFrom;
use std::fs;
use std::io::prelude::*;
use std::io::{self, SeekFrom};
use std::marker;
use std::path::Path;

use crate::entry::{EntryFields, EntryIo};
use crate::error::TarError;
use crate::header::BLOCK_SIZE;
use crate::other;
use crate::pax::*;
use crate::{Entry, GnuExtSparseHeader, GnuSparseHeader, Header};

/// A top-level representation of an archive file.
///
/// This archive can have an entry added to it and it can be iterated over.
pub struct Archive<R: ?Sized + Read> {
    inner: ArchiveInner<R>,
}

pub struct ArchiveInner<R: ?Sized> {
    pos: Cell<u64>,
    mask: u32,
    unpack_xattrs: bool,
    preserve_permissions: bool,
    preserve_ownerships: bool,
    preserve_mtime: bool,
    overwrite: bool,
    ignore_zeros: bool,
    max_metadata_size: Option<u64>,
    obj: RefCell<R>,
}

/// An iterator over the entries of an archive.
pub struct Entries<'a, R: 'a + Read> {
    fields: EntriesFields<'a>,
    _ignored: marker::PhantomData<&'a Archive<R>>,
}

trait SeekRead: Read + Seek {}
impl<R: Read + Seek> SeekRead for R {}

struct EntriesFields<'a> {
    archive: &'a Archive<dyn Read + 'a>,
    seekable_archive: Option<&'a Archive<dyn SeekRead + 'a>>,
    next: u64,
    done: bool,
    raw: bool,
}

impl<R: Read> Archive<R> {
    /// Create a new archive with the underlying object as the reader.
    pub fn new(obj: R) -> Archive<R> {
        Archive {
            inner: ArchiveInner {
                mask: u32::MIN,
                unpack_xattrs: false,
                preserve_permissions: false,
                preserve_ownerships: false,
                preserve_mtime: true,
                overwrite: true,
                ignore_zeros: false,
                max_metadata_size: None,
                obj: RefCell::new(obj),
                pos: Cell::new(0),
            },
        }
    }

    /// Unwrap this archive, returning the underlying object.
    pub fn into_inner(self) -> R {
        self.inner.obj.into_inner()
    }

    /// Construct an iterator over the entries in this archive.
    ///
    /// Note that care must be taken to consider each entry within an archive in
    /// sequence. If entries are processed out of sequence (from what the
    /// iterator returns), then the contents read for each entry may be
    /// corrupted.
    pub fn entries(&mut self) -> io::Result<Entries<'_, R>> {
        let me: &mut Archive<dyn Read> = self;
        me._entries(None).map(|fields| Entries {
            fields,
            _ignored: marker::PhantomData,
        })
    }

    /// Unpacks the contents tarball into the specified `dst`.
    ///
    /// This function will iterate over the entire contents of this tarball,
    /// extracting each file in turn to the location specified by the entry's
    /// path name.
    ///
    /// # Security
    ///
    /// See the [crate-level security documentation][crate#security]. If `dst`
    /// does not exist, it is created. Unpacking into an existing directory
    /// merges content.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::fs::File;
    /// use tar::Archive;
    ///
    /// let mut ar = Archive::new(File::open("foo.tar").unwrap());
    /// ar.unpack("foo").unwrap();
    /// ```
    pub fn unpack<P: AsRef<Path>>(&mut self, dst: P) -> io::Result<()> {
        let me: &mut Archive<dyn Read> = self;
        me._unpack(dst.as_ref())
    }

    /// Set the mask of the permission bits when unpacking this entry.
    ///
    /// The mask will be inverted when applying against a mode, similar to how
    /// `umask` works on Unix. In logical notation it looks like:
    ///
    /// ```text
    /// new_mode = old_mode & (~mask)
    /// ```
    ///
    /// The mask is 0 by default and is currently only implemented on Unix.
    pub fn set_mask(&mut self, mask: u32) {
        self.inner.mask = mask;
    }

    /// Indicate whether extended file attributes (xattrs on Unix) are preserved
    /// when unpacking this archive.
    ///
    /// This flag is disabled by default and is currently only implemented on
    /// Unix using xattr support. This may eventually be implemented for
    /// Windows, however, if other archive implementations are found which do
    /// this as well.
    pub fn set_unpack_xattrs(&mut self, unpack_xattrs: bool) {
        self.inner.unpack_xattrs = unpack_xattrs;
    }

    /// Indicate whether extended permissions (like suid on Unix) are preserved
    /// when unpacking this entry.
    ///
    /// This flag is disabled by default and is currently only implemented on
    /// Unix.
    pub fn set_preserve_permissions(&mut self, preserve: bool) {
        self.inner.preserve_permissions = preserve;
    }

    /// Indicate whether numeric ownership ids (like uid and gid on Unix)
    /// are preserved when unpacking this entry.
    ///
    /// This flag is disabled by default and is currently only implemented on
    /// Unix.
    pub fn set_preserve_ownerships(&mut self, preserve: bool) {
        self.inner.preserve_ownerships = preserve;
    }

    /// Indicate whether files and symlinks should be overwritten on extraction.
    pub fn set_overwrite(&mut self, overwrite: bool) {
        self.inner.overwrite = overwrite;
    }

    /// Indicate whether access time information is preserved when unpacking
    /// this entry.
    ///
    /// This flag is enabled by default.
    pub fn set_preserve_mtime(&mut self, preserve: bool) {
        self.inner.preserve_mtime = preserve;
    }

    /// Ignore zeroed headers, which would otherwise indicate to the archive that it has no more
    /// entries.
    ///
    /// This can be used in case multiple tar archives have been concatenated together.
    pub fn set_ignore_zeros(&mut self, ignore_zeros: bool) {
        self.inner.ignore_zeros = ignore_zeros;
    }

    /// Limits the payload size of each GNU long-name, GNU long-link, or PAX
    /// extension before it is buffered. `None` leaves metadata unrestricted.
    pub fn set_max_metadata_size(&mut self, max_metadata_size: Option<u64>) {
        self.inner.max_metadata_size = max_metadata_size;
    }

}

impl<R: Seek + Read> Archive<R> {
    /// Construct an iterator over the entries in this archive for a seekable
    /// reader. Seek will be used to efficiently skip over file contents.
    ///
    /// Note that care must be taken to consider each entry within an archive in
    /// sequence. If entries are processed out of sequence (from what the
    /// iterator returns), then the contents read for each entry may be
    /// corrupted.
    pub fn entries_with_seek(&mut self) -> io::Result<Entries<'_, R>> {
        let me: &Archive<dyn Read> = self;
        let me_seekable: &Archive<dyn SeekRead> = self;
        me._entries(Some(me_seekable)).map(|fields| Entries {
            fields,
            _ignored: marker::PhantomData,
        })
    }
}

impl Archive<dyn Read + '_> {
    fn _entries<'a>(
        &'a self,
        seekable_archive: Option<&'a Archive<dyn SeekRead + 'a>>,
    ) -> io::Result<EntriesFields<'a>> {
        if self.inner.pos.get() != 0 {
            return Err(other(
                "cannot call entries unless archive is at \
                 position 0",
            ));
        }
        Ok(EntriesFields {
            archive: self,
            seekable_archive,
            done: false,
            next: 0,
            raw: false,
        })
    }

    fn _unpack(&mut self, dst: &Path) -> io::Result<()> {
        if dst.symlink_metadata().is_err() {
            fs::create_dir_all(dst)
                .map_err(|e| TarError::new(format!("failed to create `{}`", dst.display()), e))?;
        }

        // Canonicalizing the dst directory will prepend the path with '\\?\'
        // on windows which will allow windows APIs to treat the path as an
        // extended-length path with a 32,767 character limit. Otherwise all
        // unpacked paths over 260 characters will fail on creation with a
        // NotFound exception.
        let dst = &dst.canonicalize().unwrap_or(dst.to_path_buf());

        // Delay any directory entries until the end (they will be created if needed by
        // descendants), to ensure that directory permissions do not interfere with descendant
        // extraction.
        let mut directories = Vec::new();
        for entry in self._entries(None)? {
            let mut file = entry.map_err(|e| TarError::new("failed to iterate over archive", e))?;
            if file.header().entry_type() == crate::EntryType::Directory {
                directories.push(file);
            } else {
                file.unpack_in(dst)?;
            }
        }

        // Apply the directories.
        //
        // Note: the order of application is important to permissions. That is, we must traverse
        // the filesystem graph in topological ordering or else we risk not being able to create
        // child directories within those of more restrictive permissions. See [0] for details.
        //
        // [0]: <https://github.com/alexcrichton/tar-rs/issues/242>
        directories.sort_by(|a, b| b.path_bytes().cmp(&a.path_bytes()));
        for mut dir in directories {
            dir.unpack_in(dst)?;
        }

        Ok(())
    }
}

impl<'a, R: Read> Entries<'a, R> {
    /// Indicates whether this iterator will return raw entries or not.
    ///
    /// If the raw list of entries is returned, then no preprocessing happens
    /// on account of this library, for example taking into account GNU long name
    /// or long link archive members. Raw iteration is disabled by default.
    pub fn raw(self, raw: bool) -> Entries<'a, R> {
        Entries {
            fields: EntriesFields { raw, ..self.fields },
            _ignored: marker::PhantomData,
        }
    }
}
impl<'a, R: Read> Iterator for Entries<'a, R> {
    type Item = io::Result<Entry<'a, R>>;

    fn next(&mut self) -> Option<io::Result<Entry<'a, R>>> {
        self.fields
            .next()
            .map(|result| result.map(|e| EntryFields::from(e).into_entry()))
    }
}

impl<'a> EntriesFields<'a> {
    fn next_entry_raw(
        &mut self,
        pax_extensions: Option<&[u8]>,
    ) -> io::Result<Option<Entry<'a, io::Empty>>> {
        let mut header = Header::new_old();
        let mut header_pos = self.next;
        loop {
            // Seek to the start of the next header in the archive
            let delta = self.next - self.archive.inner.pos.get();
            self.skip(delta)?;

            // EOF is an indicator that we are at the end of the archive.
            if !try_read_all(&mut &self.archive.inner, header.as_mut_bytes())? {
                return Ok(None);
            }

            // If a header is not all zeros, we have another valid header.
            // Otherwise, check if we are ignoring zeros and continue, or break as if this is the
            // end of the archive.
            if !header.as_bytes().iter().all(|i| *i == 0) {
                self.next += BLOCK_SIZE;
                break;
            }

            if !self.archive.inner.ignore_zeros {
                return Ok(None);
            }
            self.next += BLOCK_SIZE;
            header_pos = self.next;
        }

        // Make sure the checksum is ok
        let sum = header.as_bytes()[..148]
            .iter()
            .chain(&header.as_bytes()[156..])
            .fold(0, |a, b| a + (*b as u32))
            + 8 * 32;
        let cksum = header.cksum()?;
        if sum != cksum {
            return Err(other("archive header checksum mismatch"));
        }

        // PAX extensions describe the *next file entry*, not intermediary
        // extension headers (GNU LongName `L`, GNU LongLink `K`, PAX `x`/`g`).
        let entry_type = header.entry_type();
        let is_extension_header = entry_type.is_gnu_longname()
            || entry_type.is_gnu_longlink()
            || entry_type.is_pax_local_extensions()
            || entry_type.is_pax_global_extensions();

        let mut pax_size: Option<u64> = None;
        if let Some(pax_extensions_ref) = pax_extensions.filter(|_| !is_extension_header) {
            pax_size = pax_extensions_value(pax_extensions_ref, PAX_SIZE);

            if let Some(pax_uid) = pax_extensions_value(pax_extensions_ref, PAX_UID) {
                header.set_uid(pax_uid);
            }

            if let Some(pax_gid) = pax_extensions_value(pax_extensions_ref, PAX_GID) {
                header.set_gid(pax_gid);
            }
        }

        let file_pos = self.next;
        let mut size = header.entry_size()?;
        // If this exists, it must override the header size. Disagreement among
        // parsers allows construction of malicious archives that appear different
        // when parsed.
        if let Some(pax_size) = pax_size {
            size = pax_size;
        }
        if is_extension_header {
            if let Some(limit) = self.archive.inner.max_metadata_size {
                if size > limit {
                    return Err(other(&format!(
                        "tar metadata entry of {} bytes exceeds the {}-byte limit",
                        size, limit
                    )));
                }
            }
        }
        let ret = EntryFields {
            size,
            header_pos,
            file_pos,
            data: vec![EntryIo::Data((&self.archive.inner).take(size))],
            header,
            long_pathname: None,
            long_linkname: None,
            pax_extensions: None,
            mask: self.archive.inner.mask,
            unpack_xattrs: self.archive.inner.unpack_xattrs,
            preserve_permissions: self.archive.inner.preserve_permissions,
            preserve_mtime: self.archive.inner.preserve_mtime,
            overwrite: self.archive.inner.overwrite,
            preserve_ownerships: self.archive.inner.preserve_ownerships,
        };

        // Store where the next entry is, rounding up by 512 bytes (the size of
        // a header);
        let size = size
            .checked_add(BLOCK_SIZE - 1)
            .ok_or_else(|| other("size overflow"))?;
        self.next = self
            .next
            .checked_add(size & !(BLOCK_SIZE - 1))
            .ok_or_else(|| other("size overflow"))?;

        Ok(Some(ret.into_entry()))
    }

    fn next_entry(&mut self) -> io::Result<Option<Entry<'a, io::Empty>>> {
        if self.raw {
            return self.next_entry_raw(None);
        }

        let mut gnu_longname = None;
        let mut gnu_longlink = None;
        let mut pax_extensions = None;
        let mut processed = 0;
        loop {
            processed += 1;
            let entry = match self.next_entry_raw(pax_extensions.as_deref())? {
                Some(entry) => entry,
                None if processed > 1 => {
                    return Err(other(
                        "members found describing a future member \
                         but no future member found",
                    ));
                }
                None => return Ok(None),
            };

            let is_recognized_header =
                entry.header().as_gnu().is_some() || entry.header().as_ustar().is_some();

            if is_recognized_header && entry.header().entry_type().is_gnu_longname() {
                if gnu_longname.is_some() {
                    return Err(other(
                        "two long name entries describing \
                         the same member",
                    ));
                }
                gnu_longname = Some(EntryFields::from(entry).read_all()?);
                continue;
            }

            if is_recognized_header && entry.header().entry_type().is_gnu_longlink() {
                if gnu_longlink.is_some() {
                    return Err(other(
                        "two long name entries describing \
                         the same member",
                    ));
                }
                gnu_longlink = Some(EntryFields::from(entry).read_all()?);
                continue;
            }

            if is_recognized_header && entry.header().entry_type().is_pax_local_extensions() {
                if pax_extensions.is_some() {
                    return Err(other(
                        "two pax extensions entries describing \
                         the same member",
                    ));
                }
                pax_extensions = Some(EntryFields::from(entry).read_all()?);
                continue;
            }

            let mut fields = EntryFields::from(entry);
            fields.long_pathname = gnu_longname;
            fields.long_linkname = gnu_longlink;
            fields.pax_extensions = pax_extensions;
            self.parse_sparse_header(&mut fields)?;
            return Ok(Some(fields.into_entry()));
        }
    }

    fn parse_sparse_header(&mut self, entry: &mut EntryFields<'a>) -> io::Result<()> {
        if !entry.header.entry_type().is_gnu_sparse() {
            return Ok(());
        }
        let gnu = match entry.header.as_gnu() {
            Some(gnu) => gnu,
            None => return Err(other("sparse entry type listed but not GNU header")),
        };

        // Sparse files are represented internally as a list of blocks that are
        // read. Blocks are either a bunch of 0's or they're data from the
        // underlying archive.
        //
        // Blocks of a sparse file are described by the `GnuSparseHeader`
        // structure, some of which are contained in `GnuHeader` but some of
        // which may also be contained after the first header in further
        // headers.
        //
        // We read off all the blocks here and use the `add_block` function to
        // incrementally add them to the list of I/O block (in `entry.data`).
        // The `add_block` function also validates that each chunk comes after
        // the previous, we don't overrun the end of the file, and each block is
        // aligned to a 512-byte boundary in the archive itself.
        //
        // At the end we verify that the sparse file size (`Header::size`) is
        // the same as the current offset (described by the list of blocks) as
        // well as the amount of data read equals the size of the entry
        // (`Header::entry_size`).
        entry.data.truncate(0);

        let mut cur = 0;
        let mut remaining = entry.size;
        {
            let data = &mut entry.data;
            let reader = &self.archive.inner;
            let size = entry.size;
            let mut add_block = |block: &GnuSparseHeader| -> io::Result<_> {
                if block.is_empty() {
                    return Ok(());
                }
                let off = block.offset()?;
                let len = block.length()?;
                if len != 0 && (size - remaining) % BLOCK_SIZE != 0 {
                    return Err(other(
                        "previous block in sparse file was not \
                         aligned to 512-byte boundary",
                    ));
                } else if off < cur {
                    return Err(other(
                        "out of order or overlapping sparse \
                         blocks",
                    ));
                } else if cur < off {
                    let block = io::repeat(0).take(off - cur);
                    data.push(EntryIo::Pad(block));
                }
                cur = off
                    .checked_add(len)
                    .ok_or_else(|| other("more bytes listed in sparse file than u64 can hold"))?;
                remaining = remaining.checked_sub(len).ok_or_else(|| {
                    other(
                        "sparse file consumed more data than the header \
                         listed",
                    )
                })?;
                data.push(EntryIo::Data(reader.take(len)));
                Ok(())
            };
            for block in gnu.sparse.iter() {
                add_block(block)?
            }
            if gnu.is_extended() {
                let mut ext = GnuExtSparseHeader::new();
                ext.isextended[0] = 1;
                let mut metadata_bytes = 0u64;
                while ext.is_extended() {
                    metadata_bytes = metadata_bytes.checked_add(BLOCK_SIZE)
                        .ok_or_else(|| other("sparse metadata size overflow"))?;
                    if let Some(limit) = self.archive.inner.max_metadata_size {
                        if metadata_bytes > limit {
                            return Err(other(&format!(
                                "tar sparse metadata exceeds the {}-byte limit", limit
                            )));
                        }
                    }
                    if !try_read_all(&mut &self.archive.inner, ext.as_mut_bytes())? {
                        return Err(other("failed to read extension"));
                    }

                    self.next += BLOCK_SIZE;
                    for block in ext.sparse.iter() {
                        add_block(block)?;
                    }
                }
            }
        }
        if cur != gnu.real_size()? {
            return Err(other(
                "mismatch in sparse file chunks and \
                 size in header",
            ));
        }
        entry.size = cur;
        if remaining > 0 {
            return Err(other(
                "mismatch in sparse file chunks and \
                 entry size in header",
            ));
        }
        Ok(())
    }

    fn skip(&mut self, mut amt: u64) -> io::Result<()> {
        if let Some(seekable_archive) = self.seekable_archive {
            let pos = io::SeekFrom::Current(
                i64::try_from(amt).map_err(|_| other("seek position out of bounds"))?,
            );
            (&seekable_archive.inner).seek(pos)?;
        } else {
            let mut buf = [0u8; 4096 * 8];
            while amt > 0 {
                let n = cmp::min(amt, buf.len() as u64);
                let n = (&self.archive.inner).read(&mut buf[..n as usize])?;
                if n == 0 {
                    return Err(other("unexpected EOF during skip"));
                }
                amt -= n as u64;
            }
        }
        Ok(())
    }
}

impl<'a> Iterator for EntriesFields<'a> {
    type Item = io::Result<Entry<'a, io::Empty>>;

    fn next(&mut self) -> Option<io::Result<Entry<'a, io::Empty>>> {
        if self.done {
            None
        } else {
            match self.next_entry() {
                Ok(Some(e)) => Some(Ok(e)),
                Ok(None) => {
                    self.done = true;
                    None
                }
                Err(e) => {
                    self.done = true;
                    Some(Err(e))
                }
            }
        }
    }
}

impl<R: ?Sized + Read> Read for &ArchiveInner<R> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        let i = self.obj.borrow_mut().read(into)?;
        self.pos.set(self.pos.get() + i as u64);
        Ok(i)
    }
}

impl<R: ?Sized + Seek> Seek for &ArchiveInner<R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let pos = self.obj.borrow_mut().seek(pos)?;
        self.pos.set(pos);
        Ok(pos)
    }
}

/// Try to fill the buffer from the reader.
///
/// If the reader reaches its end before filling the buffer at all, returns `false`.
/// Otherwise returns `true`.
fn try_read_all<R: Read>(r: &mut R, buf: &mut [u8]) -> io::Result<bool> {
    let mut read = 0;
    while read < buf.len() {
        match r.read(&mut buf[read..])? {
            0 => {
                if read == 0 {
                    return Ok(false);
                }

                return Err(other("failed to read entire block"));
            }
            n => read += n,
        }
    }
    Ok(true)
}
