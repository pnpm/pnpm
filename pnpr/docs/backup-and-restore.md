---
id: backup-and-restore
title: Backup and restore
---

This guide covers backing up authoritative package data and authentication records, restoring them after an incident or during a migration, and verifying data integrity.

## What to back up

A pnpr instance stores two categories of data:

1. **Authoritative data (required)**:
   - **Hosted storage (`storage`)**: Contains authoritative package metadata and tarball blobs. Back this up regularly.
   - **Authentication and user state**:
     - `htpasswd` file (when configured).
     - SQLite token database (`tokens.db`, when configured).
     - If using a networked auth backend (PostgreSQL, MySQL, or Turso/libsql), back up the database according to provider documentation.
2. **Disposable cache (`cache` / `.pnpr-cache`) (optional)**:
   - Upstream proxy packument and tarball cache.
   - Resolver query cache.
   - Local artifact staging scratch.
   - **Do not back this up routinely**: The upstream proxy cache is self-healing and repopulates automatically on demand. However, for the S3 backend, in-progress publish journal data and local staging scratch live in the cache directory (`cache`, or `--cache`). Ensure pnpr has completed startup journal recovery and drained in-flight publishes before wiping cache scratch or decommissioning an instance.

## Local filesystem storage

### Backup

For local filesystem storage, perform a snapshot or atomic backup of the hosted directory and auth files:

```sh
# Export a consistent dump of hosted packages and auth records
pnpr export --out /mnt/backups/pnpr-$(date +%Y%m%d) --include-auth

# Or snapshot the storage directory directly
tar -czf /mnt/backups/pnpr-storage-$(date +%Y%m%d).tar.gz -C /var/lib/pnpr storage htpasswd tokens.db
```

### Restore

To restore:

1. Stop the running pnpr server instance.
2. Restore the files into the designated storage and auth paths:
   ```sh
   tar -xzf /mnt/backups/pnpr-storage-20261016.tar.gz -C /var/lib/pnpr
   ```
3. Run `pnpr verify` to confirm all package documents and tarballs match:
   ```sh
   pnpr verify
   ```
4. Start pnpr.

## S3 and object storage

When hosted storage is configured with S3 (`s3:` in YAML configuration), blobs and documents live in your S3-compatible bucket.

### Recommended S3 settings

- **S3 Bucket Versioning**: Enable versioning on the bucket so overwritten or deleted documents and tarballs can be recovered.
- **Cross-Region Replication**: For disaster recovery across regions, enable S3 bucket replication.
- **Lifecycle rules**: Do not set lifecycle expiration on active packages in hosted storage.

### S3 journal caveats

pnpr uses a commit journal for hosted publishes. For the S3 backend:
- The local publish journal lives beside staged blobs in the local cache directory (`cache`, or `--cache`), coordinating write-ahead publication before promotion to S3. Ensure instances stop cleanly and startup recovery completes before discarding local cache state.
- Revision digest records (`.revisions/`) live in the S3 bucket itself. When restoring an S3 bucket from a point-in-time snapshot, restore the namespaces' `.revisions/` prefixes alongside their `packages/` to keep revision digest references intact.
- After a restore, run `pnpr verify` to audit whether any references point to missing blobs:
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

# Prune unreferenced/orphaned blobs left behind by interrupted uploads
pnpr verify --prune-orphans
```

`pnpr verify` checks:
- **Missing blobs**: Tarballs referenced in a package document's version metadata that do not exist in storage.
- **Corrupted blobs**: Tarballs whose content hashes do not match the declared `shasum` or `integrity`.
- **Orphaned blobs**: Physical files in the store that are not referenced by any version of any hosted package document.

## Migrating storage to S3

To migrate hosted packages from a local directory to an S3 bucket:

```sh
# Dry run: check what would be transferred
pnpr migrate-storage --to-bucket my-registry-bucket --dry-run

# Execute migration and compare each uploaded blob with its source
pnpr migrate-storage --to-bucket my-registry-bucket --verify
```

After migration completes, update your `pnpr.yaml` to point to the S3 bucket:

```yaml
s3:
  bucket: my-registry-bucket
  region: us-east-1
```

## Importing htpasswd users

If migrating from Verdaccio or an Apache htpasswd file containing legacy `{SHA}` passwords, use `pnpr import-htpasswd`:

```sh
# Dry run: preview entries to import
pnpr import-htpasswd --source /path/to/old/htpasswd --dry-run

# Import into pnpr's htpasswd file
pnpr import-htpasswd --source /path/to/old/htpasswd
```

pnpr accepts `{SHA}` password hashes at runtime and automatically upgrades them to `$2y$` bcrypt upon the user's first successful authentication.
