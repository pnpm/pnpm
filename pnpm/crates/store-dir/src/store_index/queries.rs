use super::{HashMap, HashSet, PackageFilesIndex, StoreIndex, StoreIndexError, decode_index_value};

/// Per-query placeholder cap for [`StoreIndex::get_many`]. `SQLite`'s
/// `SQLITE_MAX_VARIABLE_NUMBER` defaulted to 999 before 3.32.0 and is
/// 32766 in newer builds (rusqlite ships a recent `SQLite`, so the
/// effective cap is well above any realistic lockfile size). Capping
/// at 999 here keeps us safe against hand-rolled custom builds with
/// the legacy default — no realistic install hits this boundary, but
/// the chunking adds maybe a microsecond of overhead and removes the
/// need to think about the cap on the read path.
pub(super) const GET_MANY_CHUNK: usize = 999;

fn escape_like_pattern(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

impl StoreIndex {
    /// Look up a package-files index by key. Returns `Ok(None)` if no row exists.
    ///
    /// Rows come in three flavours and all three decode through one
    /// path:
    /// 1. **pnpm-written**: msgpackr-records, what pnpm's
    ///    `Packr({useRecords: true, …})` emits.
    /// 2. **pacquet-written**: also msgpackr-records, from
    ///    [`encode_package_files_index`][crate::msgpackr_records::encode_package_files_index]
    ///    — pacquet matches pnpm's on-wire shape so the two tools can
    ///    share `index.db`.
    /// 3. **Legacy pacquet-written**: plain `MessagePack` maps from the
    ///    `rmp_serde::to_vec_named` path. These may still live in
    ///    caches written by older pacquet versions.
    ///
    /// All three route through
    /// [`transcode_to_plain_msgpack`][crate::msgpackr_records::transcode_to_plain_msgpack],
    /// which expands records into plain msgpack maps and narrows the
    /// `float 64` encoding of `checkedAt` back to `uint 64`. Plain
    /// msgpack rows skip the records-expansion (the `records_mode` flag
    /// never flips) but still benefit from the float narrowing. The
    /// result feeds `rmp_serde` to produce a [`PackageFilesIndex`].
    ///
    /// Cost is one `Vec<u8>` allocation + memcpy per read, dwarfed by
    /// the `SQLite` query and disk I/O.
    pub fn get(&self, key: &str) -> Result<Option<PackageFilesIndex>, StoreIndexError> {
        let row: Option<Vec<u8>> = self
            .conn
            .query_row("SELECT data FROM package_index WHERE key = ?", [key], |row| {
                row.get::<_, Vec<u8>>(0)
            })
            .map(Some)
            .or_else(|err| match err {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(StoreIndexError::Read { source: other }),
            })?;

        let Some(bytes) = row else { return Ok(None) };
        decode_index_value(&bytes).map(Some)
    }

    /// Look up the first package-files index whose key ends with `\t{pkg_id}`.
    ///
    /// This is for read-only inspection commands that only know the local
    /// package id and must not resolve against a registry to recover the
    /// integrity half of the key.
    pub fn get_by_pkg_id(
        &self,
        pkg_id: &str,
    ) -> Result<Option<PackageFilesIndex>, StoreIndexError> {
        let pattern = format!("%\t{}", escape_like_pattern(pkg_id));
        let row: Option<Vec<u8>> = self
            .conn
            .query_row(
                r"SELECT data FROM package_index WHERE key LIKE ?1 ESCAPE '\' ORDER BY key LIMIT 1",
                [pattern],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .map(Some)
            .or_else(|err| match err {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(StoreIndexError::Read { source: other }),
            })?;

        let Some(bytes) = row else { return Ok(None) };
        decode_index_value(&bytes).map(Some)
    }

    /// Look up many keys in one trip across the `SQLite` mutex.
    ///
    /// Returns a `key → PackageFilesIndex` map for every row that exists
    /// and decodes cleanly. Missing keys are simply absent from the map;
    /// rows whose msgpack payload fails to decode are logged at `debug!`
    /// and dropped, matching `load_cached_cas_paths`'s `.ok()?` stance on
    /// the per-key path — a malformed row is treated as a cache miss so
    /// the install falls through to a fresh download.
    ///
    /// `SQLite` walks the `package_index` PK B-tree once per chunk, so the
    /// per-key query overhead (≈40 µs even for misses) collapses into
    /// one round-trip. With 1352 cache keys against an empty store this
    /// drops the prefetch cost from ~50 ms of N selects to a single
    /// query — see [#294] for the cold-cache regression this fixes.
    ///
    /// [#294]: https://github.com/pnpm/pacquet/issues/294
    pub fn get_many(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, PackageFilesIndex>, StoreIndexError> {
        let raw = self.get_many_raw(keys)?;
        let mut out = HashMap::with_capacity(raw.len());
        for (key, bytes) in raw {
            match decode_index_value(&bytes) {
                Ok(entry) => {
                    out.insert(key, entry);
                }
                Err(error) => tracing::debug!(
                    target: "pacquet::store_index",
                    ?key,
                    ?error,
                    "skipping undecodable package_index row in get_many",
                ),
            }
        }
        Ok(out)
    }

    /// Batched read that returns the **undecoded** value bytes for each
    /// hit. Callers run `decode_index_value` themselves — either inline
    /// (like [`Self::get_many`]) or, more usefully, in parallel across
    /// a rayon pool after releasing the [`SharedReadonlyStoreIndex`](super::SharedReadonlyStoreIndex)
    /// mutex.
    ///
    /// The decode is the dominant CPU cost of [`Self::get_many`] for
    /// rows that carry a `manifest` field — msgpackr-records transcode
    /// plus a `rmp_serde::from_slice` of a nested JSON tree per row,
    /// times ~1k rows on a real lockfile. Doing that work under the
    /// [`SharedReadonlyStoreIndex`](super::SharedReadonlyStoreIndex) lock serialises N installs back to
    /// one thread; doing it after the lock releases lets each prefetch
    /// fan out across the rayon pool.
    pub fn get_many_raw(&self, keys: &[String]) -> Result<Vec<(String, Vec<u8>)>, StoreIndexError> {
        let mut out = Vec::with_capacity(keys.len());
        if keys.is_empty() {
            return Ok(out);
        }
        for chunk in keys.chunks(GET_MANY_CHUNK) {
            // Build a `?,?,...?` list whose length matches `chunk`. The
            // only thing interpolated into `sql` is this fixed-shape
            // placeholder string — no caller-supplied bytes ever reach
            // the SQL text. The keys themselves flow through
            // `rusqlite::params_from_iter` below, which routes them via
            // SQLite's prepared-statement parameter binding (the same
            // path every other site in this file uses). Keep the two
            // lines in lock-step: if the placeholder count or the params
            // iterator ever stop matching `chunk.len()`, that's the bug
            // to look at — not SQL injection.
            let placeholders = std::iter::repeat_n("?", chunk.len()).collect::<Vec<_>>().join(",");
            let sql = format!("SELECT key, data FROM package_index WHERE key IN ({placeholders})");
            let mut stmt =
                self.conn.prepare(&sql).map_err(|source| StoreIndexError::Read { source })?;
            let params = rusqlite::params_from_iter(chunk.iter().map(String::as_str));
            let rows = stmt
                .query_map(params, |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)))
                .map_err(|source| StoreIndexError::Read { source })?;
            for row in rows {
                let row = row.map_err(|source| StoreIndexError::Read { source })?;
                out.push(row);
            }
        }
        Ok(out)
    }

    /// Visit every raw `package_index` row without first collecting the
    /// full key set, leaving decode policy to the caller.
    pub fn for_each_raw<VisitError>(
        &self,
        mut visit: impl FnMut(String, Vec<u8>) -> Result<(), VisitError>,
    ) -> Result<(), VisitError>
    where
        VisitError: From<StoreIndexError>,
    {
        let mut stmt = self
            .conn
            .prepare("SELECT key, data FROM package_index")
            .map_err(|source| VisitError::from(StoreIndexError::Read { source }))?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)))
            .map_err(|source| VisitError::from(StoreIndexError::Read { source }))?;
        for row in rows {
            let (key, data) =
                row.map_err(|source| VisitError::from(StoreIndexError::Read { source }))?;
            visit(key, data)?;
        }
        Ok(())
    }

    /// Batched existence probe: the subset of `keys` that have a row in
    /// `package_index`. Same chunked `WHERE key IN` shape (and SQL-injection
    /// posture) as [`Self::get_many_raw`], but selects only the key column,
    /// so no row data is copied — for callers that decide *whether* to do
    /// work per key and leave reading the row to a later pass.
    pub fn contains_many(&self, keys: &[String]) -> Result<HashSet<String>, StoreIndexError> {
        let mut out = HashSet::with_capacity(keys.len());
        if keys.is_empty() {
            return Ok(out);
        }
        for chunk in keys.chunks(GET_MANY_CHUNK) {
            let placeholders = std::iter::repeat_n("?", chunk.len()).collect::<Vec<_>>().join(",");
            let sql = format!("SELECT key FROM package_index WHERE key IN ({placeholders})");
            let mut stmt =
                self.conn.prepare(&sql).map_err(|source| StoreIndexError::Read { source })?;
            let params = rusqlite::params_from_iter(chunk.iter().map(String::as_str));
            let rows = stmt
                .query_map(params, |row| row.get::<_, String>(0))
                .map_err(|source| StoreIndexError::Read { source })?;
            for key in rows {
                out.insert(key.map_err(|source| StoreIndexError::Read { source })?);
            }
        }
        Ok(out)
    }

    /// Insert or replace a package-files index.
    ///
    /// Uses the [`encode_package_files_index`][crate::msgpackr_records::encode_package_files_index]
    /// encoder, which emits msgpackr-records bytes that pnpm's
    /// `Packr({useRecords: true, moreTypes: true}).unpack(…)` reads as
    /// the same shape it produces itself. A naive
    /// `rmp_serde::to_vec_named` here produced bytes that pnpm's reader
    /// interpreted as a top-level JS `Map`, making `pkgIndex.files` a
    /// property-access miss and crashing with `files is not iterable`
    /// inside pnpm's CAFS layer.
    pub fn set(&self, key: &str, value: &PackageFilesIndex) -> Result<(), StoreIndexError> {
        let buf = crate::msgpackr_records::encode_package_files_index(value)
            .map_err(|source| StoreIndexError::Encode { source })?;
        self.conn
            .execute(
                "INSERT OR REPLACE INTO package_index (key, data) VALUES (?1, ?2)",
                rusqlite::params![key, buf],
            )
            .map(|_| ())
            .map_err(|source| StoreIndexError::Write { source })
    }

    /// Insert-or-replace a batch of rows in a single transaction.
    ///
    /// One `BEGIN IMMEDIATE` ... `COMMIT` wraps the inserts, which amortizes the
    /// WAL commit fsync across the whole batch. At 1352 snapshots that
    /// turns 1352 per-row fsyncs into ⌈`1352/batch_size`⌉ — on APFS this is
    /// the single biggest lever for `pacquet install` wall time ([#263]).
    ///
    /// `SQLite` errors during the transaction roll it back before returning,
    /// so a partial apply never leaves the index in a half-written state.
    /// A per-row msgpack encoding error is logged at `warn!` and skipped
    /// — one malformed [`PackageFilesIndex`] shouldn't cost every other row
    /// in the batch the chance to commit, matching the "best-effort
    /// index" stance the writer task and the read path already take.
    /// Encoding is done up front into a `Vec<(String, Vec<u8>)>` so the
    /// transaction body is pure `SQLite` — the caller's producer thread pays
    /// the msgpack cost, not the single writer thread.
    ///
    /// [#263]: https://github.com/pnpm/pacquet/issues/263
    pub fn set_many(
        &mut self,
        entries: impl IntoIterator<Item = (String, PackageFilesIndex)>,
    ) -> Result<(), StoreIndexError> {
        // Encode outside the transaction so a single malformed row can't
        // hold `BEGIN IMMEDIATE`'s write lock while we serialize msgpack,
        // and skip individual encoding failures with a log so one bad
        // entry doesn't drop the rest of the batch on the floor.
        let mut encoded: Vec<(String, Vec<u8>)> = Vec::new();
        for (key, value) in entries {
            match crate::msgpackr_records::encode_package_files_index(&value) {
                Ok(buf) => encoded.push((key, buf)),
                Err(source) => tracing::warn!(
                    target: "pacquet::store_index",
                    ?key,
                    error = ?source,
                    "failed to encode package_index row; skipping",
                ),
            }
        }
        if encoded.is_empty() {
            return Ok(());
        }

        let tx = self
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|source| StoreIndexError::Write { source })?;
        {
            let mut stmt = tx
                .prepare_cached("INSERT OR REPLACE INTO package_index (key, data) VALUES (?1, ?2)")
                .map_err(|source| StoreIndexError::Write { source })?;
            for (key, buf) in &encoded {
                stmt.execute(rusqlite::params![key, buf])
                    .map_err(|source| StoreIndexError::Write { source })?;
            }
        }
        tx.commit().map_err(|source| StoreIndexError::Write { source })
    }

    /// `true` iff a row with this key exists.
    pub fn contains_key(&self, key: &str) -> Result<bool, StoreIndexError> {
        let exists = self
            .conn
            .query_row("SELECT 1 FROM package_index WHERE key = ?", [key], |_| Ok(()))
            .map(|()| true)
            .or_else(|err| match err {
                rusqlite::Error::QueryReturnedNoRows => Ok(false),
                other => Err(StoreIndexError::Read { source: other }),
            })?;
        Ok(exists)
    }

    /// Collect every key in `package_index`. Useful for tests and store-prune.
    /// Buffers to avoid holding a statement borrow across the returned vector.
    pub fn keys(&self) -> Result<Vec<String>, StoreIndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT key FROM package_index")
            .map_err(|source| StoreIndexError::Read { source })?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|source| StoreIndexError::Read { source })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|source| StoreIndexError::Read { source })?);
        }
        Ok(out)
    }
}
