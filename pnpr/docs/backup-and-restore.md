---
id: backup-and-restore
title: Backup and restore
---

This guide covers backing up authoritative package data and authentication records, restoring them after an incident or during a migration, and verifying data integrity.

## What to back up

A pnpr instance stores two categories of data:

1. **Authoritative data (required)**:
   - **Hosted storage (`storage`)**: Contains the archives, stored by the identity of their bytes rather than by file name. Only the metadata database maps a package's files to them, so a copy of the store is usable only with the metadata from the same point in time. Back this up regularly.
   - **Metadata database** ([`metadata`](storage.md#sqlite-metadata)): holds the package metadata, in SQLite by default or in a libSQL database. SQLite's backup API copies the database consistently, but not at the same point in time as the hosted store, and a publish writes its archives before it commits the metadata that references them. Stop pnpr before copying a SQLite database and the hosted store, or take both copies from the same point in time, and run `pnpr verify` after a restore. Back a libSQL database up according to provider documentation.
   - **Authentication and user state**:
     - `users.json` file (when configured).
     - SQLite token database (`tokens.db`, when configured).
     - If using a networked auth backend (PostgreSQL, MySQL, or Turso/libsql), back up the database according to provider documentation.
2. **Disposable cache (`cache` / `.pnpr-cache`) (optional)**:
   - Upstream proxy packument and tarball cache.
   - Resolver query cache.
   - Local artifact staging scratch.
   - **Do not back this up routinely**: The upstream proxy cache is self-healing and repopulates automatically on demand. However, for the S3 backend, local staging scratch lives in the cache directory (`cache`, or `--cache`). Drain in-flight publishes before wiping cache scratch or decommissioning an instance.

## Local filesystem storage

### Backup

For local filesystem storage, back up the storage directory, the SQLite metadata database, and the auth files together. The default metadata database, `<storage>/.metadata/metadata.sqlite`, is inside the storage directory. A database configured with `metadata.url` elsewhere needs its own copy. Stop pnpr first, so the copy is consistent and includes the database's `-wal` file:

```sh
tar -czf /mnt/backups/pnpr-storage-$(date +%Y%m%d).tar.gz -C /var/lib/pnpr storage users.json tokens.db
```

To back up while pnpr runs, copy the metadata database with SQLite's backup API instead of copying its file. For the default database of the storage directory above:

```sh
sqlite3 /var/lib/pnpr/storage/.metadata/metadata.sqlite ".backup /mnt/backups/pnpr-metadata-$(date +%Y%m%d).sqlite"
```

With `metadata.url`, use the path that URL names.

`pnpr export` writes the current package documents, tarballs, and auth records to a directory:

```sh
pnpr export --out /mnt/backups/pnpr-$(date +%Y%m%d) --include-auth
```

An export is not a complete backup of the metadata. It leaves out three things only the database holds: the unpublished versions pnpr never accepts again, the record of every dist-tag move (its versions, time, and principal), and release notes.

### Restore

To restore:

1. Stop the running pnpr server instance.
2. Restore the files into the designated storage and auth paths:
   ```sh
   tar -xzf /mnt/backups/pnpr-storage-20261016.tar.gz -C /var/lib/pnpr
   ```
3. Restore the metadata database if the storage archive does not hold it, as with a copy made by SQLite's backup API or a database outside the storage directory. Copy it to the path `metadata.url` names, or to `<storage>/.metadata/metadata.sqlite` by default. Delete any `-wal` and `-shm` files beside that path first, so SQLite does not apply another copy's write-ahead log to it.
4. Run `pnpr verify` to confirm all package documents and tarballs match:
   ```sh
   pnpr verify
   ```
5. Start pnpr.

## S3 and object storage

When hosted storage is configured with S3 (`s3:` in YAML configuration), blobs live in your S3-compatible bucket and package metadata in the [libSQL database](storage.md#libsql-metadata) `metadata.url` names. Back them up together.

### Recommended S3 settings

- **S3 Bucket Versioning**: Enable versioning on the bucket so overwritten or deleted tarballs can be recovered.
- **Cross-Region Replication**: For disaster recovery across regions, enable S3 bucket replication.
- **Lifecycle rules**: Do not set lifecycle expiration on active packages in hosted storage.

### After a restore

- Restore the bucket and the metadata database from the same point in time, so the metadata references only blobs the bucket holds.
- Run `pnpr verify` to audit whether any references point to missing blobs:
  ```sh
  pnpr verify
  ```

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
   deployment will use, for example by importing the SQLite database
   (`<storage>/.metadata/metadata.sqlite` by default) into Turso.
2. Point `metadata.url` at that database, still without an `s3:` block:

   ```yaml
   metadata:
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

4. Add the `s3:` block, keeping `metadata.url`:

   ```yaml
   s3:
     bucket: my-registry-bucket
     region: us-east-1
   metadata:
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
