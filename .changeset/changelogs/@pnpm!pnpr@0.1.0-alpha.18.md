## 0.1.0-alpha.18

pnpr 0.1.0-alpha.18 keeps hosted package metadata in SQLite or a shared libSQL database, logs release records in transparency logs, and accepts packages with platform variants. The `documents` metadata backend is gone, so packages hosted by an earlier pnpr have to be published again.

### Minor Changes

#### Metadata storage

- pnpr now keeps hosted package metadata in SQL only. By default it uses a SQLite database at `<storage>/.metadata/metadata.sqlite`. The `metadata` block names another database by URL, such as `url: sqlite:./metadata.sqlite`. The `documents` metadata backend is removed, and a config that sets `metadata.backend` or `metadata.path` is refused.

  pnpr does not import the metadata of an existing store. A store that kept its metadata as documents starts with empty metadata, so republish its packages after upgrading. An `s3:` store that hosts packages needs a libSQL `metadata.url`. `migrate-storage` moves archives only and needs that libSQL database to hold the metadata already.

- pnpr can keep hosted package metadata in a libSQL database, such as Turso, shared by several pnpr instances. Set `metadata.url` to a `libsql://` URL. It works with an `s3:` block, so archives stay in object storage. With `metadata.replicaPath`, each instance reads package documents from a local replica that syncs every `metadata.syncInterval`. Permission checks and the reads a write depends on always read the primary.

- pnpr refuses any change to a published npm version's manifest, and never accepts an unpublished version again. pnpr records who moved each dist-tag, and when. Hosted npm package responses carry a `pnpr-generation` header.

- A reader sees all of a publish or batch publish, or none of it. A retried publish gets its original response instead of a 409. Batch publishes accept an `Idempotency-Key` header. pnpr records each publish as a publication and serves it at `/-/publications/<org>/<id-or-name>`.

- pnpr stores release notes for package versions and publications. A publish can include them in a `notes` field. Publishers can edit or delete a note later. npm clients read a version's note at `/npm/<name>/<version>/notes`. A package's notes are listed at `/npm/<name>/-/notes`. A publication's note is at `/-/publications/<org>/<id-or-name>/notes`.

#### Publishing

- A hosted pnpr registry rejects a publish whose version metadata disagrees with the `package.json` inside its archive. Its `400` response names the package, the version, and each differing field. pnpr checks the dependencies, the platform fields, `bin`, and the install scripts. pnpr also rejects a publish whose archive does not decode or holds more than one `package.json`.

  pnpr accepts the rewrites that npm and older clients make while publishing, such as a `bin` string expanded into an object. pnpr publishes a version without semver build metadata. pnpr serves a version's checked fields as its archive states them, with `hasInstallScript` and `_hasShrinkwrap` always set. Other fields, such as `description`, are served as submitted.

- pnpr accepts release archives uploaded by integrity before the publish that references them, through `PUT /-/pnpr/v0/artifacts/blob?integrity=<sha512-…>` without an owner [#17608](https://github.com/pnpm/pnpm/pull/17608). An npm publish, alone or in a batch, can then reference a staged archive: the attachment carries the archive's `integrity` in place of `data`. A version and its platform variants therefore no longer have to fit in one request body. An archive the publishing account has not staged, or one that has expired, fails the whole publish.

- Trusted-publishing workload credentials can stage archives for the packages they are configured for, and send batch publish commits for those packages. A batch entry for any other package, or of another ecosystem, is refused.

- A hosted registry that declares `tarballs: tar` now stores each published version as one tar compressed with zstd at level 19, and no `.tgz`. A client that accepts zstd gets those bytes, so even its first download is at level 19. Any other client gets the tar decoded, gzip-encoded if it asks for gzip.

  pnpr now refuses to start when a router without `tarballs: tar` lists such a registry as a source. Versions that an earlier pnpr published to such a registry are no longer served. Publish them again.

#### Package documents and platform variants

- pnpr serves its own package document to clients that ask for `application/vnd.pnpr.install-v2+json`. A large document leaves out the manifests of rarely picked versions. It serves each version's release record at `<name>/<version>+r<revision>`. npm's documents served by pnpr no longer carry `dist.pnpr`, `dist.revision`, or `dist.revisions`. The new `packageDocuments` setting configures the split threshold and the publish-time window.

- pnpr accepts platform variants with a published version and serves each one at `<name>@<version>/variants/<tag>`. Only pnpr's own package document lists them, so npm clients install the version's own archive.

- pnpr accepts a version published with platform variants only, without an archive for every other host. It generates the archive that npm's documents point to, whose `package.json` lists the variants' platforms, so npm reports an unsupported platform instead of installing an empty package.

- pnpr rejects a version published with a provenance statement that does not name each of its archives by its package URL and SHA-512. A platform variant's archive is named by the version's package URL with a `variant` qualifier, which its release record entry lists as `purl`.

#### Release records and transparency logs

- A hosted npm version is accepted with its release record, and `<name>/<version>+r0` serves the record's bytes as they were accepted. The record starts with the fields every release record has: `schema`, `purl`, `revision`, `interpretation`, the profile its manifest was derived under, and `inputs`, the digests of the `.tgz` and the `package.json` it was derived from. Each platform variant lists its own `inputs`.

- pnpr logs the release records of the npm versions it hosts. Each hosted organization has a transparency log per ecosystem, in the C2SP tiled log format. A version's log entry is recorded in the same transaction that accepts the version. With `releaseRecords.logSigningKey` set, pnpr then adds the entry to the log's tiles and signs a new checkpoint, keeping both in the hosted store.

- A hosted registry serves the transparency log of its release records under `/-/log/`: the latest checkpoint, the hash tiles, and, for callers who may read every package of the organization, the entry bundles. A release record's evidence, `<name>/<version>+r<revision>/evidence`, holds the record's digest, the inclusion proof of its log entry against the log's checkpoint, and the version's provenance bundle. `GET /-/pnpr` lists `records` and `manifestProfiles` when the records are logged.

- With `releaseRecords.statementSigningKey` set, a release record's evidence carries the registry's acceptance statement: a signed in-toto Statement saying the registry accepted the record under its profile. pnpr signs it from the accepted record each time it is read, so rotating the key signs every statement again.

### Patch Changes

- A resolve request keeps plain lockfile keys for packages from its default registry, even when a prefix such as `npmjs` points at that registry [#17575](https://github.com/pnpm/pnpm/issues/17575).

- pnpr no longer frees a package name when a package deletion fails partway. pnpr no longer restores an unpublished version when it finishes an interrupted publish at startup.

- `pnpr config check` now prints the S3 key prefix as plain text, such as `prefix: pnpr/`, and leaves it out when none is set. It printed `prefix: Some("pnpr")` or `prefix: None`.
