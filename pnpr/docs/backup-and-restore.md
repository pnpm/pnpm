---
id: backup-and-restore
title: Backup and restore
---

This guide covers backing up authoritative package data and authentication records, restoring them after an incident or during a migration, and verifying data integrity.

## What to back up

A pnpr instance stores two categories of data:

1. **Authoritative data (required)**:
   - **Hosted storage (`storage`)**: Contains package archives stored by content digest, staged publish bodies (`.staged/`), and transparency log tiles, entry bundles, and checkpoints (`.transparency-log/`). Back this up regularly.
   - **[Database](storage.md#database) (`storage.database`)**: Holds package metadata, former maintainers, unpublished versions, and transparency log entries. It also stores the [registries kept in the database](configuration.md#registries-in-the-database) with their revisions, SCIM users, and staged-publish approval records in SQLite, libSQL, PostgreSQL, or MySQL.
   - **Accounts and tokens** live in the database too, so its backup covers them. The database holds password and token hashes, so keep its backups readable only by the operator.
2. **Disposable cache (`cache` / `.pnpr-cache`) (optional)**:
   - Upstream proxy packument and tarball cache.
   - Resolver query cache.
   - Local artifact staging scratch.
   - **Do not back this up routinely**: The upstream proxy cache is self-healing and repopulates automatically on demand. However, for the S3 backend, local staging scratch lives in the cache directory (`cache`, or `--cache`). Drain in-flight publishes before wiping cache scratch or decommissioning an instance.

## Snapshot and restore order {#snapshot-order}

A publish writes archives to hosted storage before committing metadata in the database. Transparency log entry bundles, hash tiles, and checkpoints live under `.transparency-log/` in hosted storage, sequenced from the database's log entries. Managed databases and object stores cannot be snapshotted at one atomic instant. Online backups must therefore make hosted storage at least as recent as the database:

1. **Pause scheduled `pnpr gc` jobs and package unpublishes, and let any running ones finish** before starting step 2. Garbage collection and unpublish operations reclaim unmapped archives immediately, which could delete an archive still referenced by the database snapshot.
2. **Back up the database first.** Record the backup timestamp ($T_{\text{db}}$) when using point-in-time recovery.
3. **Block new publishes, let in-flight publishes and log integration finish, and capture hosted storage second** ($T_{\text{storage}} \ge T_{\text{db}}$). Draining publishes and integration ensures a signed checkpoint covers every entry bundle written after $T_{\text{db}}$.
4. **Resume publishes, `pnpr gc`, and package unpublishes** once the storage snapshot or S3 restore timestamp is recorded.

Why this order keeps both archives and the transparency log consistent:

- Every archive referenced at $T_{\text{db}}$ was uploaded before $T_{\text{db}}$ and stays in hosted storage at $T_{\text{storage}}$ while `pnpr gc` and unpublishes are paused.
- If publishes reached `.transparency-log/` between $T_{\text{db}}$ and $T_{\text{storage}}$, their signed checkpoint authenticates the stored entry bundles. pnpr re-imports those entries before serving, marks rolled-back npm versions removed so they cannot be reused, and extends the same Merkle tree.
- Never restore hosted storage to an **older** state than the database ($T_{\text{storage}} < T_{\text{db}}$). An older store lacks archives and integrated log tiles that the database already references. pnpr refuses to start or sign checkpoints when `.transparency-log/` lacks entry bundles for its tree or diverges from the database's log.

## Local filesystem storage

### Backup

To back up while pnpr is stopped, stop pnpr and any scheduled `pnpr gc` jobs first so the archive includes a consistent SQLite database and its `-wal` file:

```sh
tar -czf "/mnt/backups/pnpr-storage-$(date +%Y%m%d).tar.gz" -C /var/lib/pnpr storage
```

The default database, `<storage>/.metadata/metadata.sqlite`, is inside the storage directory. A SQLite file outside `<storage>` needs its own copy, and a PostgreSQL, MySQL, or libSQL `storage.database.url` needs a database snapshot before archiving storage.

To back up while pnpr runs, your storage volume must provide point-in-time filesystem snapshots (such as ZFS, Btrfs, LVM, or cloud block-volume snapshots). A live `tar` walk across a changing directory can copy a newly written log checkpoint without the tiles and entry bundles written earlier in the walk. Without filesystem snapshots, stop pnpr or block writes and let background log integration finish before archiving.

With filesystem snapshots, follow the [snapshot order](#snapshot-order):

1. Pause scheduled `pnpr gc` jobs and block package unpublishes, then wait for any running `pnpr gc` or unpublish operations to finish.
2. Back up the database first:
   ```sh
   sqlite3 /var/lib/pnpr/storage/.metadata/metadata.sqlite ".backup /mnt/backups/pnpr-metadata-$(date +%Y%m%d).sqlite"
   ```
   For a `sqlite:` `storage.database.url`, pass the path that URL names. For a PostgreSQL, MySQL, or libSQL `storage.database.url`, take a provider snapshot or record a point-in-time recovery timestamp ($T_{\text{db}}$) instead.
3. Block new publishes, let in-flight publishes finish, and wait for background log integration to complete. Take a point-in-time snapshot of `/var/lib/pnpr` ($T_{\text{storage}} \ge T_{\text{db}}$), resume publishes, mount the snapshot read-only at `/mnt/pnpr-snap`, and archive hosted storage:
   ```sh
   tar --exclude='storage/.metadata' -czf "/mnt/backups/pnpr-storage-$(date +%Y%m%d).tar.gz" -C /mnt/pnpr-snap storage
   ```
4. Unmount and release the filesystem snapshot, then resume scheduled `pnpr gc` jobs and unpublishes.

Back up the database and hosted storage together. pnpr has no full logical
export/import command for restoring an instance or switching database backends.
`pnpr registries export` and `pnpr registries import` transfer only the stored
registry configuration, not packages or accounts.

### Restore

To restore local filesystem storage:

1. Stop all running pnpr instances and scheduled `pnpr gc` jobs.
2. Move aside or remove any existing `storage` directory so newer log bundles or stale SQLite write-ahead logs do not remain. Extract hosted storage into `/var/lib/pnpr`:
   ```sh
   rm -rf /var/lib/pnpr/storage
   tar -xzf /mnt/backups/pnpr-storage-20261016.tar.gz -C /var/lib/pnpr
   ```
3. Restore the database so the storage archive comes from the same offline backup or a snapshot captured **after** $T_{\text{db}}$:
   - **SQLite**: Keep any `-wal` file extracted from an offline `tar` archive because it can hold committed transactions. When restoring a standalone `sqlite3 .backup` file or an external `sqlite:` database, delete destination `-wal` and `-shm` files before copying it into place.
   - **PostgreSQL, MySQL, or libSQL (`storage.database.url`)**: Restore the database to $T_{\text{db}}$. When `replicaPath` is configured for libSQL, delete that local replica file and its `-wal` and `-shm` files before starting pnpr.
4. Run `pnpr verify` to confirm all referenced package documents and archives match:
   ```sh
   pnpr verify
   ```
   Archives uploaded between the metadata backup and the storage backup appear as orphaned blobs; they do not fail verification.
5. Start pnpr. Startup reconciles `.transparency-log/` against the database and re-imports any log entries bundled after the metadata backup. pnpr refuses to start if `.transparency-log/` lacks entry bundles for its tree or diverges from the database's log.
6. Re-enable `pnpr gc` (or run `pnpr gc` once) to reclaim unreferenced archives uploaded after the metadata backup.

## S3 and object storage

With S3 hosted storage (`storage.s3` in YAML configuration), archives and `.transparency-log/` live in your bucket. Package metadata lives in the libSQL, PostgreSQL, or MySQL database selected by `storage.database.url`.

### Recommended S3 settings

- **S3 Bucket Versioning**: Enable versioning on the bucket so deleted or overwritten objects can be restored within your recovery window. Configure lifecycle rules to expire noncurrent versions after that window and remove expired delete markers. `pnpr gc` deletes current objects; versioning retains their bytes until those rules expire them.
- **Cross-Region Replication**: For disaster recovery across regions, enable S3 bucket replication.
- **Lifecycle rules**: Do not set lifecycle expiration on active objects in hosted storage or `.transparency-log/`. Retain noncurrent object versions and delete markers across your full backup retention window so `pnpr gc` and unpublishes stay recoverable.

### Coordinated backup

1. Pause scheduled `pnpr gc` jobs and block package unpublishes until step 3 finishes. Wait for any running `pnpr gc` or unpublish operations to complete before step 2.
2. Take a snapshot or record the point-in-time recovery timestamp ($T_{\text{db}}$) of the database (`storage.database.url`).
3. Block new publishes, let in-flight publishes finish, and wait for background log integration to complete. Record a restore timestamp ($T_{\text{bucket}} \ge T_{\text{db}}$) on the versioned S3 bucket. Do not copy a live bucket without versioning unless writes are stopped. A live object walk can copy a newer log checkpoint without the tiles and bundles written earlier.
4. Resume publishes, scheduled `pnpr gc` jobs, and package unpublishes.

### Restore

1. Stop all running pnpr replicas and scheduled `pnpr gc` jobs.
2. Restore the database to $T_{\text{db}}$ and restore the S3 bucket to $T_{\text{bucket}} \ge T_{\text{db}}$ (never earlier than $T_{\text{db}}$). If an unpublish ran between $T_{\text{db}}$ and $T_{\text{bucket}}$, also restore the archive object versions deleted during that window.
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
2. Point `storage.database.url` at that database, still without a `storage.s3` block:

   ```yaml
   storage:
     database:
       url: libsql://<your-db>.turso.io
       authToken: ${PNPR_LIBSQL_TOKEN}
   ```

3. Move the archives and transparency log. `migrate-storage` copies every stored
   archive and `.transparency-log/` record under the same key, and refuses
   metadata an S3 deployment cannot read:

   ```sh
   # Dry run: check what would be transferred
   pnpr migrate-storage --to-bucket my-registry-bucket --dry-run

   # Execute migration and compare each uploaded blob with its source
   pnpr migrate-storage --to-bucket my-registry-bucket --verify
   ```

4. Add the `storage.s3` block, keeping `storage.database.url`:

   ```yaml
   storage:
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
