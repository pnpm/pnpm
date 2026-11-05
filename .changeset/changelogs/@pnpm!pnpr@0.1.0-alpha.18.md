## 0.1.0-alpha.18

### Minor Changes

- With SQLite metadata, a reader sees all of a publish or batch publish, or none of it. A retried publish gets its original response instead of a 409. Batch publishes accept an `Idempotency-Key` header. pnpr records each publish as a publication and serves it at `/-/publications/<org>/<id-or-name>`.

- pnpr can keep hosted package metadata in a libSQL database, such as Turso, shared by several pnpr instances. Set `metadata.url` to a `libsql://` URL. It works with an `s3:` block, so archives stay in object storage. With `metadata.replicaPath`, each instance reads package documents from a local replica that syncs every `metadata.syncInterval`. Permission checks and the reads a write depends on always read the primary.

- With `releaseRecords.statementSigningKey` set, a release record's evidence carries the registry's acceptance statement: a signed in-toto Statement saying the registry accepted the record under its profile. pnpr signs it from the accepted record each time it is read, so rotating the key signs every statement again.

- A hosted registry that declares `tarballs: tar` now stores each published version as one tar compressed with zstd at level 19, and no `.tgz`. A client that accepts zstd gets those bytes, so even its first download is at level 19. Any other client gets the tar decoded, gzip-encoded if it asks for gzip.

  pnpr now refuses to start when a router without `tarballs: tar` lists such a registry as a source. Versions that an earlier pnpr published to such a registry are no longer served. Publish them again.

- pnpm downloads less metadata from a registry declared with `serverType: pnpr`. It asks such a registry for pnpr's own package document. A large document leaves out the manifests of versions that are rarely picked. pnpm fetches such a manifest only when it picks that version. In a benchmark that resolves 787 packages, the package documents shrank from 30.3 MB to 2.5 MB gzipped. pnpm caches these documents in `v12/metadata-pnpr`, and the `pnpm cache` commands cover them.

  pnpm warns when a registry that is not declared `serverType: pnpr` turns out to be pnpr. For a registry declared `serverType: pnpr`, a `minimumReleaseAge` longer than 30 days fails with `ERR_PNPM_UNSUPPORTED_MINIMUM_RELEASE_AGE`.

  pnpr serves its own package document to clients that ask for `application/vnd.pnpr.install-v2+json`. It serves each version's release record at `<name>/<version>+r<revision>`. npm's documents served by pnpr no longer carry `dist.pnpr`, `dist.revision`, or `dist.revisions`. The new `packageDocuments` setting configures the split threshold and the publish-time window.

- pnpm installs the platform variants of packages from a registry declared with `serverType: pnpr`. The lockfile records every variant of a package, and pnpm links the one that fits the host, or the package's own archive when none does.

  pnpr accepts platform variants with a published version and serves each one at `<name>@<version>/variants/<tag>`. Only pnpr's own package document lists them, so npm clients install the version's own archive.

- pnpr rejects a version published with a provenance statement that does not name each of its archives by its package URL and SHA-512. A platform variant's archive is named by the version's package URL with a `variant` qualifier, which its release record entry now lists as `purl`.

- A hosted pnpr registry rejects a publish whose version metadata disagrees with the `package.json` inside its archive. Its `400` response names the package, the version, and each differing field. pnpr checks the dependencies, the platform fields, `bin`, and the install scripts. pnpr also rejects a publish whose archive does not decode or holds more than one `package.json`. pnpr accepts the rewrites that npm and older clients make while publishing, such as a `bin` string expanded into an object. pnpr publishes a version without semver build metadata. pnpr serves a version's checked fields as its archive states them, with `hasInstallScript` and `_hasShrinkwrap` always set. Other fields, such as `description`, are served as submitted.

- An npm publish to pnpr, alone or in a batch, can reference an archive staged before it: the attachment carries the archive's `integrity` in place of `data`. A version and its platform variants therefore no longer have to fit in one request body. An archive the publishing account has not staged, or one that has expired, fails the whole publish.

- With SQLite metadata, pnpr stores release notes for package versions and publications. A publish can include them in a `notes` field. Publishers can edit or delete a note later. npm clients read a version's note at `/npm/<name>/<version>/notes`. A package's notes are listed at `/npm/<name>/-/notes`. A publication's note is at `/-/publications/<org>/<id-or-name>/notes`.

- With SQLite or libSQL metadata, a hosted npm version is accepted with its release record, and `<name>/<version>+r0` serves the record's bytes as they were accepted. The record starts with the fields every release record has: `schema`, `purl`, `revision`, `interpretation`, the profile its manifest was derived under, and `inputs`, the digests of the `.tgz` and the `package.json` it was derived from. Each platform variant lists its own `inputs`. The metadata database's schema changed, so a database written by an earlier pnpr is refused.

- A hosted registry serves the transparency log of its release records under `/-/log/`: the latest checkpoint, the hash tiles, and, for callers who may read every package of the organization, the entry bundles. A release record's evidence, `<name>/<version>+r<revision>/evidence`, holds the record's digest, the inclusion proof of its log entry against the log's checkpoint, and the version's provenance bundle. `GET /-/pnpr` lists `records` and `manifestProfiles` when the records are logged.

- pnpr logs the release records of the npm versions it hosts with SQLite or libSQL metadata. Each hosted organization has a transparency log per ecosystem, in the C2SP tiled log format. A version's log entry is recorded in the same transaction that accepts the version. With `releaseRecords.logSigningKey` set, pnpr then adds the entry to the log's tiles and signs a new checkpoint, keeping both in the hosted store. The metadata database's schema changed, so a database written by an earlier pnpr is refused.

- Removed the `documents` metadata backend. pnpr keeps package metadata only in SQL: the SQLite database in the storage directory by default, or the database `metadata.url` names. A store that kept its metadata as documents starts with empty metadata. A config that sets `metadata.backend` is refused, and an `s3:` store that hosts packages needs a libSQL `metadata.url`. `migrate-storage` moves archives only and needs that libSQL database to hold the metadata already.

- pnpr accepts release archives uploaded by integrity before the publish that references them, through `PUT /-/pnpr/v0/artifacts/blob?integrity=<sha512-…>` without an owner [#17608](https://github.com/pnpm/pnpm/pull/17608).

- pnpr accepts a version published with platform variants only, without an archive for every other host. It generates the archive that npm's documents point to, whose `package.json` lists the variants' platforms, so npm reports an unsupported platform instead of installing an empty package.

- Trusted-publishing workload credentials can stage archives for the packages they are configured for. They can also send batch publish commits for those packages. A batch entry for any other package, or of another ecosystem, is refused.

- pnpr now keeps hosted package metadata in SQLite by default, at `<storage>/.metadata/metadata.sqlite`. The `metadata` block names the database by URL, such as `url: sqlite:./metadata.sqlite`. The `metadata` block no longer accepts `backend: sqlite` or `path`. A registry with an `s3:` block and no metadata URL keeps package documents in the bucket. pnpr does not import the package documents of an existing filesystem store. Republish its packages after upgrading.

- pnpr can keep hosted package metadata in an embedded SQLite database. With SQLite metadata, pnpr refuses any change to a published npm version's manifest. pnpr never accepts an unpublished version again, even when its tombstone is lost. pnpr records who moved each dist-tag, and when. Hosted npm package responses carry a `pnpr-generation` header.

### Patch Changes

- A resolve request keeps plain lockfile keys for packages from its default registry, even when a prefix such as `npmjs` points at that registry [#17575](https://github.com/pnpm/pnpm/issues/17575).

- `pnpr config check` now prints the S3 key prefix as plain text, such as `prefix: pnpr/`, and leaves it out when none is set. It printed `prefix: Some("pnpr")` or `prefix: None`.

- pnpr refuses a SQL metadata database of another schema version before it creates any table in it. The error names the version pnpr expects and what the database holds, and says to wipe pnpr's metadata tables, which pnpr does not migrate before 1.0.

- Instances sharing a libSQL metadata database now accept both of two concurrent publishes to one package by the same owners: the second adds its versions and tags to what the first committed. A publish refused at its commit now keeps its archives on libSQL.

- `--storage` now moves the default SQLite metadata database along with the storage directory. Before, pnpr kept reading the database under the storage directory of the config file. A database `metadata.url` names anywhere else stays where it is.

- pnpr no longer frees a package name when a package deletion fails partway. pnpr no longer restores an unpublished version when it finishes an interrupted publish at startup.
