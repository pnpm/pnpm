use std::{
    cell::{
        Cell,
        RefCell,
    },
    cmp,
    convert::TryFrom,
    fs,
    io::{
        self,
        SeekFrom,
        prelude::*,
    },
    marker,
    path::Path,
};

use crate::{
    Entry,
    GnuSparseHeader,
    Header,
    agreement,
    entry::{
        EntryFields,
        EntryIo,
    },
    error::TarError,
    header::BLOCK_SIZE,
    other,
    pax::*,
};

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
    read_like_node_tar: bool,
    ended_at_end_marker: Cell<bool>,
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
    /// Whether a header was read, and whether a block was skipped as one
    /// node-tar cannot read.
    read_header: bool,
    skipped_block: bool,
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
                read_like_node_tar: false,
                ended_at_end_marker: Cell::new(false),
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
        me._entries(None)
            .map(|fields| Entries { fields, _ignored: marker::PhantomData })
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

    /// Reads two header shapes the way node-tar does instead of refusing
    /// them: a directory entry has no data whatever its size, and the last
    /// of two PAX records with one key wins. Also refuses a GNU long-name,
    /// GNU long-link, or PAX entry over 1 MiB, node-tar's default
    /// `maxMetaEntrySize`, which node-tar skips without applying. Off by
    /// default.
    pub fn set_read_like_node_tar(&mut self, read_like_node_tar: bool) {
        self.inner.read_like_node_tar = read_like_node_tar;
    }

    /// Whether entry iteration stopped at an end-of-archive marker: two
    /// zero blocks. `false` while entries remain, and when the input ended
    /// at a header boundary or after a single zero block.
    pub fn ended_at_end_marker(&self) -> bool {
        self.inner.ended_at_end_marker.get()
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
        me._entries(Some(me_seekable))
            .map(|fields| Entries { fields, _ignored: marker::PhantomData })
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
            read_header: false,
            skipped_block: false,
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
        Entries { fields: EntriesFields { raw, ..self.fields }, _ignored: marker::PhantomData }
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
        let read_like_node_tar = self.archive.inner.read_like_node_tar;
        let mut saw_null_block = false;
        loop {
            // Seek to the start of the next header in the archive
            let delta = self.next - self.archive.inner.pos.get();
            self.skip(delta)?;

            // EOF is an indicator that we are at the end of the archive.
            if !try_read_all(&mut &self.archive.inner, header.as_mut_bytes())? {
                return self.end_of_input();
            }

            self.next += BLOCK_SIZE;
            // If a header is not a zero block, we have another header.
            // Otherwise, check if we are ignoring zeros and continue, or break as if this is the
            // end of the archive.
            let block = header.as_bytes();
            if read_like_node_tar && agreement::is_null_block(block) {
                // node-tar ends the archive at two null blocks in a row, and
                // reads a header after a single one.
                if saw_null_block && !self.archive.inner.ignore_zeros {
                    self.archive.inner.ended_at_end_marker.set(true);
                    return self.end_of_input();
                }
                saw_null_block = true;
                continue;
            }
            saw_null_block = false;
            if read_like_node_tar {
                // node-tar skips a block it cannot read as a header.
                if !Self::reads_as_node_tar_header(block, pax_extensions)? {
                    self.skipped_block = true;
                    continue;
                }
                break;
            }
            if !block.iter().all(|byte| *byte == 0) {
                break;
            }
            if !self.archive.inner.ignore_zeros {
                return self.end_at_zero_block();
            }
        }
        let header_pos = self.next - BLOCK_SIZE;
        self.read_header = true;

        if !self.archive.inner.read_like_node_tar {
            let sum = header.as_bytes()[..148]
                .iter()
                .chain(&header.as_bytes()[156..])
                .fold(0, |a, b| a + (*b as u32))
                + 8 * 32;
            let cksum = header.cksum()?;
            if sum != cksum {
                return Err(other("archive header checksum mismatch"));
            }
        }
        let header_names =
            self.archive.inner.read_like_node_tar.then(|| agreement::header_names(&header));
        match &header_names {
            Some((path, link_name)) => {
                agreement::check_header_block(&header, (path, link_name.as_deref()))?;
            }
            None => agreement::check_header_block(
                &header,
                (&header.path_bytes(), header.link_name_bytes().as_deref()),
            )?,
        }

        // PAX extensions describe the *next file entry*, not intermediary
        // extension headers (GNU LongName `L`, GNU LongLink `K`, PAX `x`/`g`).
        let entry_type = header.entry_type();
        let is_extension_header = agreement::is_gnu_long_name_type(entry_type.as_byte())
            || entry_type.is_gnu_longlink()
            || entry_type.is_pax_local_extensions()
            || entry_type.is_pax_global_extensions();

        let mut pax_size: Option<u64> = None;
        if let Some(pax_extensions_ref) = pax_extensions.filter(|_| !is_extension_header) {
            pax_size = agreement::pax_size(pax_extensions_ref)?;

            if let Some(pax_uid) = pax_extensions_value(pax_extensions_ref, PAX_UID) {
                header.set_uid(pax_uid);
            }

            if let Some(pax_gid) = pax_extensions_value(pax_extensions_ref, PAX_GID) {
                header.set_gid(pax_gid);
            }
        }

        let file_pos = self.next;
        // If this exists, it must override the header size. Disagreement among
        // parsers allows construction of malicious archives that appear different
        // when parsed.
        let mut size = match pax_size {
            Some(pax_size) => pax_size,
            None if self.archive.inner.read_like_node_tar => {
                agreement::header_size(&header.as_old().size)
                    .ok_or_else(|| other("archive header has a size node-tar cannot read"))?
            }
            None => header.entry_size()?,
        };
        if !is_extension_header {
            if self.archive.inner.read_like_node_tar && header.entry_type().is_dir() {
                size = 0;
            }
            agreement::check_entry_size(&header, size)?;
        }
        if is_extension_header {
            if let Some(limit) = self.metadata_size_limit() {
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
            header_names,
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
        self.next = self.next
            .checked_add(size & !(BLOCK_SIZE - 1))
            .ok_or_else(|| other("size overflow"))?;

        Ok(Some(ret.into_entry()))
    }

    /// Ends the entries. An archive whose every block node-tar skipped is not
    /// a tar.
    fn end_of_input(&self) -> io::Result<Option<Entry<'a, io::Empty>>> {
        if self.skipped_block && !self.read_header {
            return Err(other("archive has no header that node-tar can read"));
        }
        Ok(None)
    }

    /// Whether node-tar reads `block` as a header, rather than skipping it.
    /// node-tar does not decode the size field of an entry that a pending
    /// PAX `size` applies to.
    fn reads_as_node_tar_header(
        block: &[u8; BLOCK_SIZE as usize],
        pax_extensions: Option<&[u8]>,
    ) -> io::Result<bool> {
        if !agreement::is_valid_header_block(block) {
            return Ok(false);
        }
        let pax_sets_size = match pax_extensions {
            Some(records) if agreement::is_normal_fs_type(block[156]) => {
                agreement::pax_size(records)?.is_some()
            }
            _ => false,
        };
        Ok(pax_sets_size || agreement::header_size(&block[124..136]).is_some())
    }

    /// Ends the archive at a zero block, unless data follows it, which
    /// node-tar would read as a header.
    fn end_at_zero_block(&mut self) -> io::Result<Option<Entry<'a, io::Empty>>> {
        let mut block = Vec::with_capacity(BLOCK_SIZE as usize);
        (&self.archive.inner).take(BLOCK_SIZE).read_to_end(&mut block)?;
        if block.iter().any(|byte| *byte != 0) {
            return Err(other("archive has data after a single zero block"));
        }
        if block.len() == BLOCK_SIZE as usize {
            self.archive.inner.ended_at_end_marker.set(true);
        }
        Ok(None)
    }

    fn metadata_size_limit(&self) -> Option<u64> {
        let limit = self.archive.inner.max_metadata_size;
        if !self.archive.inner.read_like_node_tar {
            return limit;
        }
        let node_tar_limit = agreement::NODE_TAR_MAX_META_ENTRY_SIZE;
        Some(limit.map_or(node_tar_limit, |limit| limit.min(node_tar_limit)))
    }

    fn keep_last_pax_records(&self, records: Vec<u8>) -> io::Result<Vec<u8>> {
        if self.archive.inner.read_like_node_tar {
            agreement::keep_last_pax_records(&records)
        } else {
            Ok(records)
        }
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
            let entry_type = entry.header().entry_type();
            let is_extension_type = agreement::is_gnu_long_name_type(entry_type.as_byte())
                || entry_type.is_gnu_longlink()
                || entry_type.is_pax_local_extensions()
                || entry_type.is_pax_global_extensions();
            if is_extension_type && !is_recognized_header {
                return Err(other("extension header in an archive header without ustar magic"));
            }
            let has_pending_extension =
                gnu_longname.is_some() || gnu_longlink.is_some() || pax_extensions.is_some();

            if entry_type.is_pax_global_extensions() {
                if has_pending_extension {
                    return Err(other(
                        "global PAX extended header between an extension header \
                         and the member it describes",
                    ));
                }
                let mut fields = EntryFields::from(entry);
                let records = fields.read_all()?;
                let records = self.keep_last_pax_records(records)?;
                agreement::check_pax_records(&records, true)?;
                fields.pax_extensions = Some(records);
                return Ok(Some(fields.into_entry()));
            }

            if is_recognized_header && agreement::is_gnu_long_name_type(entry_type.as_byte()) {
                if gnu_longname.is_some() {
                    return Err(other(
                        "two long name entries describing \
                         the same member",
                    ));
                }
                let value = agreement::node_tar_string(&EntryFields::from(entry).read_all()?);
                gnu_longname = Some(value);
                continue;
            }

            if is_recognized_header
                && entry
                    .header()
                    .entry_type()
                    .is_gnu_longlink()
            {
                if gnu_longlink.is_some() {
                    return Err(other(
                        "two long name entries describing \
                         the same member",
                    ));
                }
                let value = agreement::node_tar_string(&EntryFields::from(entry).read_all()?);
                gnu_longlink = Some(value);
                continue;
            }

            if is_recognized_header
                && entry
                    .header()
                    .entry_type()
                    .is_pax_local_extensions()
            {
                if pax_extensions.is_some() {
                    return Err(other(
                        "two pax extensions entries describing \
                         the same member",
                    ));
                }
                let records = EntryFields::from(entry).read_all()?;
                let records = self.keep_last_pax_records(records)?;
                agreement::check_pax_records(&records, false)?;
                pax_extensions = Some(records);
                continue;
            }

            let typeflag = entry.header().as_old().linkflag[0];
            if !agreement::is_normal_fs_type(typeflag) {
                if has_pending_extension {
                    return Err(other(
                        "extension header describes an entry type it does not apply to",
                    ));
                }
                let mut fields = EntryFields::from(entry);
                self.parse_sparse_header(&mut fields)?;
                return Ok(Some(fields.into_entry()));
            }

            let has_long_and_pax_name = {
                let pax_has = |key: &str| {
                    pax_extensions
                        .as_deref()
                        .is_some_and(|records| {
                            PaxExtensions::new(records)
                                .any(|record| record.is_ok_and(|record| record.key() == Ok(key)))
                        })
                };
                (gnu_longname.is_some() && pax_has(PAX_PATH))
                    || (gnu_longlink.is_some() && pax_has(PAX_LINKPATH))
            };
            let mut fields = EntryFields::from(entry);
            fields.long_pathname = gnu_longname;
            fields.long_linkname = gnu_longlink;
            fields.pax_extensions = pax_extensions;
            agreement::check_resolved_entry(
                typeflag,
                fields.size,
                &fields.path_bytes(),
                fields.link_name_bytes().as_deref(),
                has_long_and_pax_name,
            )?;
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
        if gnu.is_extended() {
            // node-tar reads the extension blocks as the entry's data.
            return Err(other("GNU sparse entries with extension headers are not supported"));
        }

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
                remaining = remaining
                    .checked_sub(len)
                    .ok_or_else(|| {
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
