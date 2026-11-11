---
id: backup-and-restore
title: Backup and restore
---

This guide covers backing up authoritative package data and authentication records, restoring them after an incident or during a migration, and verifying data integrity.

## What to back up

A pnpr instance stores two categories of data:

1. **Authoritative data (required)**:
   - **Hosted storage (`storage`)**: Contains package archives stored by content digest, staged publish bodies (`.staged/`), and transparency log tiles, entry bundles, and checkpoints (`.transparency-log/`). Back this up regularly.
   - **Metadata database** ([`metadata`](storage.md#sqlite-metadata)): Holds package metadata, former maintainers, unpublished versions, and transparency log entries. It also stores team rosters, rule overrides, SCIM users, and staged-publish approval records in SQLite, libSQL, PostgreSQL, or MySQL.
   - **Authentication and user state**:
     - `users.json` file (when configured).
     - SQLite token database (`tokens.db`, when configured).
     - If using a networked auth backend (PostgreSQL, MySQL, or Turso/libSQL), back up the database according to provider documentation.
2. **Disposable cache (`cache` / `.pnpr-cache`) (optional)**:
   - Upstream proxy packument and tarball cache.
   - Resolver query cache.
   - Local artifact staging scratch.
   - **Do not back this up routinely**: The upstream proxy cache is self-healing and repopulates automatically on demand. However, for the S3 backend, local staging scratch lives in the cache directory (`cache`, or `--cache`). Drain in-flight publishes before wiping cache scratch or decommissioning an instance.

## Snapshot and restore order {#snapshot-order}

A publish writes archives to hosted storage before committing metadata in the database. Transparency log entry bundles, hash tiles, and checkpoints live under `.transparency-log/` in hosted storage, sequenced from the database's log entries. Managed databases and object stores cannot be snapshotted at one atomic instant. Online backups must therefore make hosted storage at least as recent as the metadata database:

1. **Pause scheduled `pnpr gc` jobs** until both backups finish. Garbage collection removes unmapped archives from unpublished versions immediately, which could otherwise delete an archive still referenced by the database snapshot.
2. **Back up the metadata database first.** Record the backup timestamp ($T_{\text{db}}$) when using point-in-time recovery.
3. **Back up hosted storage second** (or record an S3 restore timestamp $T_{\text{storage}} \ge T_{\text{db}}$ with S3 Bucket Versioning enabled).
4. **Resume `pnpr gc`** after both backups complete.

Why this order keeps both archives and the transparency log consistent:

- Every archive referenced at $T_{\text{db}}$ was uploaded before $T_{\text{db}}$ and stays in hosted storage at $T_{\text{storage}}$ while `pnpr gc` is paused.
- If publishes reached `.transparency-log/` between $T_{\text{db}}$ and $T_{\text{storage}}$, pnpr re-imports their log entries from the stored entry bundles before serving or accepting publishes. Those rolled-back versions are marked removed so their version numbers cannot be reused, and new publishes extend the same Merkle tree without forking the log.
- Never restore hosted storage to an **older** state than the metadata database ($T_{\text{storage}} < T_{\text{db}}$). An older store lacks archives and integrated log tiles that the database already references. pnpr refuses to start or sign checkpoints when `.transparency-log/` lacks entry bundles for its tree or diverges from the database's log.

## Local filesystem storage

### Backup

To back up while pnpr is stopped, stop pnpr and any scheduled `pnpr gc` jobs first so the archive includes a consistent SQLite database and its `-wal` file:

```sh
tar -czf /mnt/backups/pnpr-storage-$(date +%Y%m%d).tar.gz -C /var/lib/pnpr storage users.json tokens.db
```

The default metadata database, `<storage>/.metadata/metadata.sqlite`, is inside the storage directory. A database configured with `database.url` outside `<storage>` needs its own copy.

To back up while pnpr runs, follow the [snapshot order](#snapshot-order):

1. Pause scheduled `pnpr gc` jobs.
2. Back up the SQLite metadata database first with SQLite's backup API:
   ```sh
   sqlite3 /var/lib/pnpr/storage/.metadata/metadata.sqlite ".backup /mnt/backups/pnpr-metadata-$(date +%Y%m%d).sqlite"
   ```
   With `database.url`, pass the path that URL names.
3. Archive the storage directory and auth files second, excluding the live SQLite database files inside `<storage>/.metadata/`:
   ```sh
   tar --exclude='storage/.metadata' -czf /mnt/backups/pnpr-storage-$(date +%Y%m%d).tar.gz -C /var/lib/pnpr storage users.json tokens.db
   ```
4. Resume scheduled `pnpr gc` jobs.

`pnpr export` writes the current package documents, tarballs, and auth records to a directory:

```sh
pnpr export --out /mnt/backups/pnpr-$(date +%Y%m%d) --include-auth
```

An export does not include transparency logs, unpublished versions, former maintainer claims, dist-tag history, release notes, team rosters, rule overrides, SCIM users, or staged publishes. Back up the [metadata database](storage.md#sqlite-metadata) and hosted storage to retain those records.

### Restore

To restore local filesystem storage:

1. Stop all running pnpr instances and scheduled `pnpr gc` jobs.
2. Restore hosted storage and auth files into their configured paths:
   ```sh
   tar -xzf /mnt/backups/pnpr-storage-20261016.tar.gz -C /var/lib/pnpr
   ```
3. Restore the metadata database when it was backed up separately (`<storage>/.metadata/metadata.sqlite` by default, or the path `database.url` names). Delete any `-wal` and `-shm` files at the destination path first so SQLite does not replay another copy's write-ahead log. Copy the `.sqlite` backup into place, and confirm the storage archive came from the same offline backup or was captured **after** the metadata backup.
4. Run `pnpr verify` to confirm all referenced package documents and archives match:
   ```sh
   pnpr verify
   ```
   Archives uploaded between the metadata backup and the storage backup appear as orphaned blobs; they do not fail verification.
5. Start pnpr. Startup reconciles `.transparency-log/` against the metadata database and re-imports any log entries bundled after the metadata backup. pnpr refuses to start if `.transparency-log/` lacks entry bundles for its tree or diverges from the database's log.
6. Re-enable `pnpr gc` (or run `pnpr gc` once) to reclaim unreferenced archives uploaded after the metadata backup.

## S3 and object storage

With S3 hosted storage (`s3:` in YAML configuration), archives and `.transparency-log/` live in your bucket. Package metadata lives in the libSQL, PostgreSQL, or MySQL database selected by `database.url`.

### Recommended S3 settings

- **S3 Bucket Versioning**: Enable versioning on the bucket so deleted or overwritten objects can be restored to a chosen timestamp.
- **Cross-Region Replication**: For disaster recovery across regions, enable S3 bucket replication.
- **Lifecycle rules**: Do not set lifecycle expiration on active objects in hosted storage or `.transparency-log/`.

### Coordinated backup

1. Pause scheduled `pnpr gc` jobs.
2. Take a snapshot or record the point-in-time recovery timestamp ($T_{\text{db}}$) of the metadata database (`database.url`).
3. Record a restore timestamp ($T_{\text{bucket}} \ge T_{\text{db}}$) on the versioned S3 bucket, or copy the bucket after the database snapshot completes.
4. Resume scheduled `pnpr gc` jobs.

### Restore

1. Stop all running pnpr replicas and scheduled `pnpr gc` jobs.
2. Restore the metadata database to $T_{\text{db}}$ and restore the S3 bucket to $T_{\text{bucket}} \ge T_{\text{db}}$ (never to a timestamp earlier than $T_{\text{db}}$).
3. Run `pnpr verify` to confirm every referenced archive exists in the bucket and matches its hash:
   ```sh
   pnpr verify
   ```
4. Start pnpr. Startup reconciles `.transparency-log/` in the bucket with the restored database and re-imports any entries bundled between $T_{\text{db}}$ and $T_{\text{bucket}}$. pnpr refuses to start if the bucket's log state diverges from the database.
5. Resume `pnpr gc` to reclaim unreferenced archives uploaded between $T_{\text{db}}$ and $T_{\text{bucket}}$.

## Verifying data integrity

Use the `pnpr verify` subcommand to audit hosted documents against physical storage blobs.

```sh
# Full checksum verification (verifies SHA-1 and sha512 integrity hashes)
pnpr verify

# Check that every packument version points to an existing blob, without comparing hashes
pnpr verify --skip-hashes

# Remove the archives no hosted package maps, such as those of interrupted uploads
pnpr gc
```

`pnpr verify` checks:
- **Missing blobs**: Tarballs referenced in a package document's version metadata that do not exist in storage.
- **Corrupted blobs**: Tarballs whose content hashes do not match the declared `shasum` or `integrity`.
- **Orphaned blobs**: Stored archives that are not the live bytes of any file of a hosted package, such as what an interrupted publish stored. `verify` only reports them; [`pnpr gc`](cli.md#gc) removes them.

## Migrating storage to S3

To migrate hosted packages from a local directory to an S3 bucket:

1. Stop pnpr, and move the package metadata into the libSQL database the S3
   deployment uses, for example by importing the SQLite database
   (`<storage>/.metadata/metadata.sqlite` by default) into Turso.
2. Point `database.url` at that database, still without an `s3:` block:

   ```yaml
   database:
     url: libsql://<your-db>.turso.io
     authToken: ${PNPR_LIBSQL_TOKEN}
   ```

3. Move the archives. `migrate-storage` copies every stored archive under the
   same key, which the metadata maps files to, and refuses metadata an S3
   deployment cannot read:

   ```sh
   # Dry run: check what would be transferred
   pnpr migrate-storage --to-bucket my-registry-bucket --dry-run

   # Execute migration and compare each uploaded blob with its source
   pnpr migrate-storage --to-bucket my-registry-bucket --verify
   ```

4. Add the `s3:` block, keeping `database.url`:

   ```yaml
   s3:
     bucket: my-registry-bucket
     region: us-east-1
   database:
     url: libsql://<your-db>.turso.io
     authToken: ${PNPR_LIBSQL_TOKEN}
   ```

## Importing htpasswd users

If migrating from Verdaccio or an Apache htpasswd file containing legacy `{SHA}` passwords, use `pnpr import-htpasswd`:

```sh
# Dry run: preview entries to import
pnpr import-htpasswd --source /path/to/old/htpasswd --dry-run

# Import into pnpr's account file
pnpr import-htpasswd --source /path/to/old/htpasswd
```

pnpr accepts `{SHA}` password hashes at runtime and automatically upgrades them to `$2y$` bcrypt upon the user's first successful authentication.
