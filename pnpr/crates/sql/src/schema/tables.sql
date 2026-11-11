-- Large payloads use rowid tables so interior index pages hold only keys.

-- Timestamps are INTEGER milliseconds since the Unix epoch. Registry-generated
-- timestamps use the application clock; published_at records the supplied event time.
-- The current generation of each hosted namespace.
CREATE TABLE IF NOT EXISTS namespaces (
    namespace TEXT NOT NULL PRIMARY KEY,
    generation INTEGER NOT NULL
) STRICT;

-- One row per package. `generation` is the namespace generation of the last
-- commit that changed it. `state_version` is the format version of `state`,
-- which holds the package-level fields no child table stores.
CREATE TABLE IF NOT EXISTS packages (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    generation INTEGER NOT NULL,
    state_version INTEGER NOT NULL,
    state BLOB NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name)
) STRICT;

-- Published versions of a Cargo crate, ordered by `position` as the sparse
-- index serves them. `entry` holds the immutable sparse-index fields; `yanked`
-- flips in place.
CREATE TABLE IF NOT EXISTS cargo_versions (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    position INTEGER NOT NULL,
    yanked INTEGER NOT NULL,
    entry TEXT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version)
) STRICT;
CREATE INDEX IF NOT EXISTS cargo_versions_by_position
    ON cargo_versions (namespace, ecosystem, name, position);

-- Published distribution files of a Python project, ordered by `position`.
-- `entry` holds the immutable PEP 691 file metadata; `yanked` and `yank_reason`
-- change in place.
CREATE TABLE IF NOT EXISTS pypi_files (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    filename TEXT NOT NULL,
    position INTEGER NOT NULL,
    yanked INTEGER NOT NULL,
    yank_reason TEXT,
    entry TEXT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, filename)
) STRICT;
CREATE INDEX IF NOT EXISTS pypi_files_by_position
    ON pypi_files (namespace, ecosystem, name, position);

-- Distribution filenames permanently reserved after deletion from a Python
-- project, so a deleted filename can never be uploaded again.
CREATE TABLE IF NOT EXISTS removed_pypi_files (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    filename TEXT NOT NULL,
    generation INTEGER NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, filename)
) STRICT, WITHOUT ROWID;

-- Manifests retained in an OCI image repository, keyed by content digest.
-- `entry` holds child digests and indexed referrer metadata.
CREATE TABLE IF NOT EXISTS oci_manifests (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    digest TEXT NOT NULL,
    entry TEXT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, digest)
) STRICT;

-- Mutable tags of an OCI image repository and the manifest digest each names.
CREATE TABLE IF NOT EXISTS oci_tags (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    tag TEXT NOT NULL,
    digest TEXT NOT NULL,
    updated INTEGER NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, tag)
) STRICT, WITHOUT ROWID;

-- Insert-only: a version's artifact integrity. `accepted_generation` is the
-- namespace generation that accepted it, which is the id of the version's
-- initial publication.
CREATE TABLE IF NOT EXISTS versions (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    integrity TEXT NOT NULL,
    published_at INTEGER,
    accepted_generation INTEGER NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version)
) STRICT, WITHOUT ROWID;

-- Insert-only: the manifest accepted for a version under an interpretation
-- profile. `inputs` is a JSON object of the digests of the raw inputs the
-- profile derived it from, such as the archive's package.json; it is empty
-- for the `submitted` profile, whose input is the manifest itself.
-- `accepted_digest` covers the manifest, README, profile, and inputs, which a
-- restatement of the version must match.
CREATE TABLE IF NOT EXISTS accepted_manifests (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    profile TEXT NOT NULL,
    manifest TEXT NOT NULL,
    inputs TEXT NOT NULL,
    accepted_digest TEXT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version, profile)
) STRICT;

-- Insert-only: the exact bytes of a version's release record, as accepted
-- under `profile` and served unchanged at its record route. `digest` is their
-- SHA-512, `sha512-<base64>`, which the record's log entry commits to, and
-- `log_index` the index of that entry in its namespace's log.
CREATE TABLE IF NOT EXISTS release_records (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    profile TEXT NOT NULL,
    record BLOB NOT NULL,
    digest TEXT NOT NULL,
    log_index INTEGER NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version, profile)
) STRICT;

-- Insert-only: the transparency log of each namespace's records of one
-- ecosystem, one row per entry, sequenced in the transaction that accepts the
-- record. `idx` counts from 0 without gaps. Kept when a version or package is
-- removed: a log only grows.
CREATE TABLE IF NOT EXISTS log_entries (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    idx INTEGER NOT NULL,
    entry BLOB NOT NULL,
    PRIMARY KEY (namespace, ecosystem, idx)
) STRICT;

-- The tree each log's tiles and checkpoint were integrated to: its size and
-- its RFC 6962 root. Integration checks the stored tiles against it before it
-- extends a log, so altered tiles are never signed over, and only moves it to
-- a larger tree.
CREATE TABLE IF NOT EXISTS log_heads (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    size INTEGER NOT NULL,
    root BLOB NOT NULL,
    PRIMARY KEY (namespace, ecosystem)
) STRICT, WITHOUT ROWID;

-- Kept out of the manifest so a README never inflates resolution metadata.
CREATE TABLE IF NOT EXISTS readmes (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    readme TEXT,
    readme_filename TEXT,
    PRIMARY KEY (namespace, ecosystem, name, version)
) STRICT;

CREATE TABLE IF NOT EXISTS tags (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    tag TEXT NOT NULL,
    version TEXT NOT NULL,
    generation INTEGER NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, tag)
) STRICT, WITHOUT ROWID;

-- Insert-only: every move of a tag, so a release-age or downgrade policy can
-- see when a tag moved, not only where it points. Kept when the package is
-- removed.
CREATE TABLE IF NOT EXISTS tag_history (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    tag TEXT NOT NULL,
    generation INTEGER NOT NULL,
    previous TEXT,
    target TEXT,
    principal TEXT,
    changed_at INTEGER NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, tag, generation)
) STRICT, WITHOUT ROWID;

-- Insert-only: the identity of every version removed from a package, by
-- unpublishing it or the whole package. Kept when the package is removed, so
-- a removed version is never accepted again and a version always names the
-- same bytes.
CREATE TABLE IF NOT EXISTS removed_versions (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    generation INTEGER NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version)
) STRICT, WITHOUT ROWID;

-- Every manifest removed from an OCI repository, by deleting the manifest or
-- removing the repository, so a publication that introduced it is not replayed
-- after its removal. Kept when the repository is removed, with the generation
-- of its latest removal when pushed and removed again.
CREATE TABLE IF NOT EXISTS removed_oci_manifests (
    namespace TEXT NOT NULL,
    name TEXT NOT NULL,
    artifact TEXT NOT NULL,
    generation INTEGER NOT NULL,
    PRIMARY KEY (namespace, name, artifact)
) STRICT, WITHOUT ROWID;

-- Mutable annotations of a version. `deprecated` is its JSON value.
-- `generation` is the generation that last changed the row.
CREATE TABLE IF NOT EXISTS annotations (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    deprecated TEXT NOT NULL,
    generation INTEGER NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version)
) STRICT, WITHOUT ROWID;

-- A commit that introduced artifacts. `id` is the namespace generation it
-- committed. `name` is the publisher's, unique within the namespace.
CREATE TABLE IF NOT EXISTS publications (
    namespace TEXT NOT NULL,
    id INTEGER NOT NULL,
    name TEXT,
    principal TEXT,
    committed_at INTEGER NOT NULL,
    PRIMARY KEY (namespace, id)
) STRICT;
CREATE UNIQUE INDEX IF NOT EXISTS publications_by_name
    ON publications (namespace, name) WHERE name IS NOT NULL;

-- Insert-only: each artifact belongs to every publication that introduced it.
-- Kept when the package is removed.
CREATE TABLE IF NOT EXISTS publication_members (
    namespace TEXT NOT NULL,
    publication INTEGER NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    artifact TEXT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version, artifact, publication)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS publication_members_by_publication
    ON publication_members (namespace, publication);

-- The outcome of a committed publication request, keyed by the principal
-- that sent it and its idempotency key, for the request's retries.
CREATE TABLE IF NOT EXISTS publication_outcomes (
    namespace TEXT NOT NULL,
    principal TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    generation INTEGER NOT NULL,
    publication INTEGER,
    committed_at INTEGER NOT NULL,
    PRIMARY KEY (namespace, principal, idempotency_key)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS publication_outcomes_by_committed_at
    ON publication_outcomes (committed_at);

-- Release notes of a package version (`kind` = 'version', `subject` = the
-- version) or of a publication (`kind` = 'publication', `ecosystem` and
-- `name` empty, `subject` = its id). `text` is NULL once deleted.
-- `initial_digest` is the digest of the text published with the subject.
CREATE TABLE IF NOT EXISTS notes (
    namespace TEXT NOT NULL,
    kind TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    subject TEXT NOT NULL,
    text TEXT,
    modified_at INTEGER NOT NULL,
    principal TEXT,
    initial_digest TEXT,
    PRIMARY KEY (namespace, kind, ecosystem, name, subject)
) STRICT, WITHOUT ROWID;

-- The bytes the hosted store holds, by identity (`<algorithm>-<hex>`), and
-- the incarnation stored under `<blob_id>.<incarnation>` now, `live`,
-- `reclaiming`, or `gone`. Never removed, so incarnations only grow and a key
-- a reclaim may remove is never written again. `changed_at` is when `state`
-- last changed.
CREATE TABLE IF NOT EXISTS blobs (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    blob_id TEXT NOT NULL,
    incarnation INTEGER NOT NULL,
    state TEXT NOT NULL,
    size INTEGER NOT NULL,
    changed_at INTEGER NOT NULL,
    PRIMARY KEY (namespace, ecosystem, blob_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS blobs_by_state ON blobs (namespace, ecosystem, state);

-- Each public file name of a package, mapped to the blob holding its bytes,
-- by the commit that publishes it or an image blob's upload, and dropped
-- with its version or package.
CREATE TABLE IF NOT EXISTS artifact_blobs (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    artifact TEXT NOT NULL,
    version TEXT NOT NULL,
    blob_id TEXT NOT NULL,
    mapped_at INTEGER NOT NULL,
    mapping TEXT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, artifact)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS artifact_blobs_by_blob_id
    ON artifact_blobs (namespace, ecosystem, blob_id);

-- The record of each archive a principal staged by the identity
-- (`<algorithm>-<hex>`) of its bytes ahead of the commit that claims it. The
-- bytes stay in the hosted store under `<blob_id>.<incarnation>`. `state` is
-- `incomplete`, `complete`, or `gone`; never removed, so incarnations only
-- grow and a key a sweep may remove is never written again. `staged_at` is
-- `0` once given back or claimed.
CREATE TABLE IF NOT EXISTS staged_archives (
    principal TEXT NOT NULL,
    blob_id TEXT NOT NULL,
    incarnation INTEGER NOT NULL,
    staged_at INTEGER NOT NULL,
    size INTEGER NOT NULL,
    state TEXT NOT NULL,
    PRIMARY KEY (principal, blob_id)
) STRICT, WITHOUT ROWID;
-- The records a principal still holds, so its usage and its sweeps read
-- them without the tombstones of every archive it ever staged.
CREATE INDEX IF NOT EXISTS staged_archives_held
    ON staged_archives (principal) WHERE state != 'gone';

-- Provisioned SCIM user accounts and deletion tombstones (`removed` = 1).
-- `deprovisions` counts transitions from active to inactive so replicas revoke
-- sessions even if an account is reactivated between polls.
CREATE TABLE IF NOT EXISTS scim_users (
    user_id TEXT NOT NULL PRIMARY KEY,
    active INTEGER NOT NULL,
    removed INTEGER NOT NULL,
    deprovisions INTEGER NOT NULL,
    attributes TEXT NOT NULL,
    generation INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS scim_users_by_generation
    ON scim_users (generation);

-- Registries whose team roster has been initialized in the database.
CREATE TABLE IF NOT EXISTS team_rosters (
    registry TEXT NOT NULL PRIMARY KEY,
    generation INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS teams (
    registry TEXT NOT NULL,
    team_id TEXT NOT NULL,
    name TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (registry, team_id)
) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX IF NOT EXISTS teams_by_name
    ON teams (registry, name);
CREATE INDEX IF NOT EXISTS teams_by_position
    ON teams (registry, position);

CREATE TABLE IF NOT EXISTS team_members (
    registry TEXT NOT NULL,
    team_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    PRIMARY KEY (registry, team_id, user_id)
) STRICT, WITHOUT ROWID;

-- Registries with stored API rule overrides, and the generation of their
-- current override set.
CREATE TABLE IF NOT EXISTS rule_override_sets (
    registry TEXT NOT NULL PRIMARY KEY,
    generation INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

-- Overridden rule lists of a registry (`pattern` = '' for the registry-level
-- default `access`, else a declared package pattern). Presence with zero rows
-- in `rule_override_grants` represents an empty access list (`[]`).
CREATE TABLE IF NOT EXISTS rule_overrides (
    registry TEXT NOT NULL,
    pattern TEXT NOT NULL,
    action TEXT NOT NULL,
    PRIMARY KEY (registry, pattern, action)
) STRICT, WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS rule_override_grants (
    registry TEXT NOT NULL,
    pattern TEXT NOT NULL,
    action TEXT NOT NULL,
    position INTEGER NOT NULL,
    principal TEXT NOT NULL,
    PRIMARY KEY (registry, pattern, action, position)
) STRICT, WITHOUT ROWID;

-- The active Cloudflare CDN cache-tag target shared by replicas of this database.
CREATE TABLE IF NOT EXISTS cdn_subscriptions (
    singleton INTEGER NOT NULL PRIMARY KEY CHECK (singleton = 1),
    target TEXT NOT NULL
) STRICT;

-- Coalesced CDN cache invalidations (`package` = '*' for a full zone-tag purge,
-- else a package name).
CREATE TABLE IF NOT EXISTS cdn_purge_queue (
    package TEXT NOT NULL PRIMARY KEY,
    generation INTEGER NOT NULL
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS cdn_purge_queue_by_generation
    ON cdn_purge_queue (generation);

CREATE TABLE IF NOT EXISTS staged_metadata (
    id TEXT PRIMARY KEY NOT NULL,
    generation INTEGER NOT NULL,
    body BLOB NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS removed_packages (
    namespace TEXT NOT NULL,
    ecosystem TEXT NOT NULL,
    name TEXT NOT NULL,
    maintainers TEXT NOT NULL,
    unpublished_at INTEGER NOT NULL,
    unpublished_by TEXT,
    PRIMARY KEY (namespace, ecosystem, name)
) STRICT, WITHOUT ROWID;

-- Generations stay unique after a record is removed and recreated.
CREATE TABLE IF NOT EXISTS record_generation (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    generation INTEGER NOT NULL
) STRICT;
INSERT INTO record_generation (singleton, generation) VALUES (1, 0)
    ON CONFLICT DO NOTHING;

-- Registered user accounts for SQL-backed auth.
CREATE TABLE IF NOT EXISTS users (
    id TEXT NOT NULL PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    username_skeleton_hash TEXT UNIQUE,
    bcrypt_hash TEXT NOT NULL,
    name_revision INTEGER NOT NULL DEFAULT 0
) STRICT;

-- Bearer tokens for SQL-backed and local SQLite auth.
CREATE TABLE IF NOT EXISTS tokens (
    token_hash TEXT NOT NULL PRIMARY KEY,
    user_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_used_at INTEGER NOT NULL,
    readonly INTEGER NOT NULL DEFAULT 0,
    cidr_whitelist TEXT NOT NULL DEFAULT '[]',
    name TEXT,
    expires_at INTEGER,
    scopes TEXT,
    kind TEXT
) STRICT;
CREATE INDEX IF NOT EXISTS tokens_user_id ON tokens (user_id);

-- Monotonic and reconciled counters for SQL-backed auth.
CREATE TABLE IF NOT EXISTS auth_counters (
    name TEXT NOT NULL PRIMARY KEY,
    value INTEGER NOT NULL
) STRICT;
