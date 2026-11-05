-- The current generation of each hosted catalog.
CREATE TABLE IF NOT EXISTS catalogs (
    catalog TEXT NOT NULL PRIMARY KEY,
    generation INTEGER NOT NULL
) STRICT;

-- One row per package. `generation` is the catalog generation of the last
-- commit that changed it. `state` holds what no other table does: for npm,
-- the package-level fields of the packument; for any other ecosystem, its
-- whole document.
CREATE TABLE IF NOT EXISTS packages (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    generation INTEGER NOT NULL,
    state BLOB NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name)
) STRICT, WITHOUT ROWID;

-- Insert-only: a version's artifact integrity. `accepted_generation` is the
-- catalog generation that accepted it, which is the id of the version's
-- initial publication.
CREATE TABLE IF NOT EXISTS versions (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    revision_key TEXT NOT NULL,
    integrity TEXT NOT NULL,
    published_at TEXT,
    accepted_generation INTEGER NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, version, revision_key),
    FOREIGN KEY (catalog, ecosystem, name)
        REFERENCES packages (catalog, ecosystem, name) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

-- Insert-only: the manifest accepted for a version under an interpretation
-- profile. `inputs` is a JSON object of the digests of the raw inputs the
-- profile derived it from, such as the archive's package.json; it is empty
-- for the `submitted` profile, whose input is the manifest itself.
-- `accepted_digest` covers the manifest, README, profile, and inputs, which a
-- restatement of the version must match.
CREATE TABLE IF NOT EXISTS accepted_manifests (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    revision_key TEXT NOT NULL,
    profile TEXT NOT NULL,
    manifest TEXT NOT NULL,
    inputs TEXT NOT NULL,
    accepted_digest TEXT NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, version, revision_key, profile),
    FOREIGN KEY (catalog, ecosystem, name, version, revision_key)
        REFERENCES versions (catalog, ecosystem, name, version, revision_key) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

-- Insert-only: the exact bytes of a version's release record, as accepted
-- under `profile` and served unchanged at its record route. `digest` is their
-- SHA-512, `sha512-<base64>`, which the record's log entry commits to, and
-- `log_index` the index of that entry in its catalog's log.
CREATE TABLE IF NOT EXISTS release_records (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    revision_key TEXT NOT NULL,
    profile TEXT NOT NULL,
    record BLOB NOT NULL,
    digest TEXT NOT NULL,
    log_index INTEGER NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, version, revision_key, profile),
    FOREIGN KEY (catalog, ecosystem, name, version, revision_key, profile)
        REFERENCES accepted_manifests (catalog, ecosystem, name, version, revision_key, profile)
        ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

-- Insert-only: the transparency log of each catalog's records of one
-- ecosystem, one row per entry, sequenced in the transaction that accepts the
-- record. `idx` counts from 0 without gaps. Kept when a version or package is
-- removed: a log only grows.
CREATE TABLE IF NOT EXISTS log_entries (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    idx INTEGER NOT NULL,
    entry BLOB NOT NULL,
    PRIMARY KEY (catalog, ecosystem, idx)
) STRICT, WITHOUT ROWID;

-- The tree each log's tiles and checkpoint were integrated to: its size and
-- its RFC 6962 root. Integration checks the stored tiles against it before it
-- extends a log, so altered tiles are never signed over, and only moves it to
-- a larger tree.
CREATE TABLE IF NOT EXISTS log_heads (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    size INTEGER NOT NULL,
    root BLOB NOT NULL,
    PRIMARY KEY (catalog, ecosystem)
) STRICT, WITHOUT ROWID;

-- Kept out of the manifest so a README never inflates resolution metadata.
CREATE TABLE IF NOT EXISTS readmes (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    revision_key TEXT NOT NULL,
    readme TEXT,
    readme_filename TEXT,
    PRIMARY KEY (catalog, ecosystem, name, version, revision_key),
    FOREIGN KEY (catalog, ecosystem, name, version, revision_key)
        REFERENCES versions (catalog, ecosystem, name, version, revision_key) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS tags (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    tag TEXT NOT NULL,
    version TEXT NOT NULL,
    generation INTEGER NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, tag),
    FOREIGN KEY (catalog, ecosystem, name)
        REFERENCES packages (catalog, ecosystem, name) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

-- Insert-only: every move of a tag, so a release-age or downgrade policy can
-- see when a tag moved, not only where it points. Kept when the package is
-- removed.
CREATE TABLE IF NOT EXISTS tag_history (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    tag TEXT NOT NULL,
    generation INTEGER NOT NULL,
    previous TEXT,
    target TEXT,
    principal TEXT,
    changed_at TEXT NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, tag, generation)
) STRICT, WITHOUT ROWID;

-- Insert-only: the identity of every version removed from a package, by
-- unpublishing it or the whole package. Kept when the package is removed, so
-- a removed version is never accepted again and a version and revision always
-- name the same bytes.
CREATE TABLE IF NOT EXISTS removed_versions (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    revision_key TEXT NOT NULL,
    generation INTEGER NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, version, revision_key)
) STRICT, WITHOUT ROWID;

-- Mutable annotations of a version. `deprecated` is its JSON value.
-- `generation` is the generation that last changed the row.
CREATE TABLE IF NOT EXISTS annotations (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    deprecated TEXT NOT NULL,
    generation INTEGER NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, version),
    FOREIGN KEY (catalog, ecosystem, name)
        REFERENCES packages (catalog, ecosystem, name) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

-- A commit that introduced artifacts. `id` is the catalog generation it
-- committed. `name` is the publisher's, unique within the catalog.
CREATE TABLE IF NOT EXISTS publications (
    catalog TEXT NOT NULL,
    id INTEGER NOT NULL,
    name TEXT,
    principal TEXT,
    committed_at TEXT NOT NULL,
    PRIMARY KEY (catalog, id)
) STRICT;
-- The versions a commit accepted, which are its publication's members.
CREATE INDEX IF NOT EXISTS versions_by_accepted_generation
    ON versions (catalog, accepted_generation);
CREATE UNIQUE INDEX IF NOT EXISTS publications_by_name
    ON publications (catalog, name) WHERE name IS NOT NULL;

-- Insert-only: each artifact belongs to the publication that introduced it.
-- Kept when the package is removed.
CREATE TABLE IF NOT EXISTS publication_members (
    catalog TEXT NOT NULL,
    publication INTEGER NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    artifact TEXT NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, version, artifact),
    FOREIGN KEY (catalog, publication) REFERENCES publications (catalog, id)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS publication_members_by_publication
    ON publication_members (catalog, publication);

-- The outcome of a committed publication request, keyed by the principal
-- that sent it and its idempotency key, for the request's retries.
CREATE TABLE IF NOT EXISTS publication_outcomes (
    catalog TEXT NOT NULL,
    principal TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    generation INTEGER NOT NULL,
    publication INTEGER,
    committed_at INTEGER NOT NULL,
    PRIMARY KEY (catalog, principal, idempotency_key)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS publication_outcomes_by_key
    ON publication_outcomes (principal, idempotency_key);

-- Release notes of a package version (`kind` = 'version', `subject` = the
-- version) or of a publication (`kind` = 'publication', `ecosystem` and
-- `name` empty, `subject` = its id). `text` is NULL once deleted.
-- `initial_digest` is the digest of the text published with the subject.
CREATE TABLE IF NOT EXISTS notes (
    catalog TEXT NOT NULL,
    kind TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    subject TEXT NOT NULL,
    text TEXT,
    modified_at TEXT NOT NULL,
    principal TEXT,
    initial_digest TEXT,
    PRIMARY KEY (catalog, kind, ecosystem, name, subject)
) STRICT, WITHOUT ROWID;

-- Insert-only: every change of a note, with the text it replaced, so an
-- accidental or unauthorized edit can be restored.
CREATE TABLE IF NOT EXISTS note_history (
    catalog TEXT NOT NULL,
    kind TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    subject TEXT NOT NULL,
    changed_at TEXT NOT NULL,
    principal TEXT,
    previous TEXT,
    next TEXT
) STRICT;

-- The bytes the hosted store holds, by identity (`<algorithm>-<hex>`), and
-- the incarnation stored under `<integrity>.<incarnation>` now, `live`,
-- `reclaiming`, or `gone`. Never removed, so incarnations only grow and a key
-- a reclaim may remove is never written again. `changed_at` is when `state`
-- last changed, in seconds since the Unix epoch.
CREATE TABLE IF NOT EXISTS blobs (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    integrity TEXT NOT NULL,
    incarnation INTEGER NOT NULL,
    state TEXT NOT NULL,
    size INTEGER NOT NULL,
    changed_at INTEGER NOT NULL,
    PRIMARY KEY (catalog, ecosystem, integrity)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS blobs_by_state ON blobs (catalog, ecosystem, state);

-- Each public file name of a package, mapped to the blob holding its bytes,
-- by the commit that publishes it or an image blob's upload, and dropped
-- with its version or package. `mapped_at` is in seconds since the epoch.
CREATE TABLE IF NOT EXISTS artifact_blobs (
    catalog TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    artifact TEXT NOT NULL,
    version TEXT NOT NULL,
    integrity TEXT NOT NULL,
    mapped_at INTEGER NOT NULL,
    mapping TEXT NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, artifact)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS artifact_blobs_by_integrity
    ON artifact_blobs (catalog, ecosystem, integrity);

-- The record of each archive a principal staged by the hex SHA-512 digest
-- of its bytes ahead of the commit that claims it. The bytes stay in the
-- hosted store under `<digest>.<incarnation>`. `state` is `incomplete`,
-- `complete`, or `gone`; never removed, so incarnations only grow and a key
-- a sweep may remove is never written again. `staged_at` is in seconds since
-- the epoch, `0` once given back or claimed.
CREATE TABLE IF NOT EXISTS staged_archives (
    principal TEXT NOT NULL,
    digest TEXT NOT NULL,
    incarnation INTEGER NOT NULL,
    staged_at INTEGER NOT NULL,
    size INTEGER NOT NULL,
    state TEXT NOT NULL,
    PRIMARY KEY (principal, digest)
) STRICT, WITHOUT ROWID;
-- The records a principal still holds, so its usage and its sweeps read
-- them without the tombstones of every archive it ever staged.
CREATE INDEX IF NOT EXISTS staged_archives_held
    ON staged_archives (principal) WHERE state != 'gone';
