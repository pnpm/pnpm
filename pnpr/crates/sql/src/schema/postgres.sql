-- Timestamps are BIGINT milliseconds since the Unix epoch. Registry-generated
-- timestamps use the application clock; published_at records the supplied event time.

-- Text columns use the "C" collation, so keys compare and sort by bytes, as
-- `SQLite`'s BINARY and `MySQL`'s VARBINARY columns do.

-- The current generation of each hosted namespace.
CREATE TABLE IF NOT EXISTS namespaces (
    namespace TEXT COLLATE "C" NOT NULL PRIMARY KEY,
    generation BIGINT NOT NULL
);

-- One row per package. `generation` is the namespace generation of the last
-- commit that changed it. `state_version` is the format version of `state`,
-- which holds the package-level fields no child table stores.
CREATE TABLE IF NOT EXISTS packages (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    generation BIGINT NOT NULL,
    state_version BIGINT NOT NULL,
    state BYTEA NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name)
);

-- The account IDs of a package's owners (`maintainers[*].userId` on npm,
-- `owners` on Cargo, PyPI, and OCI), kept in sync with `packages.state` so
-- owner checks never read or parse the package document.
CREATE TABLE IF NOT EXISTS package_owners (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    user_id TEXT COLLATE "C" NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, user_id)
);

-- Published versions of a Cargo crate, ordered by `position` as the sparse
-- index serves them. `entry` holds the immutable sparse-index fields; `yanked`
-- flips in place.
CREATE TABLE IF NOT EXISTS cargo_versions (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    version TEXT COLLATE "C" NOT NULL,
    position BIGINT NOT NULL,
    yanked BIGINT NOT NULL,
    entry TEXT COLLATE "C" NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version)
);
CREATE INDEX IF NOT EXISTS cargo_versions_by_position
    ON cargo_versions (namespace, ecosystem, name, position);

-- Published distribution files of a Python project, ordered by `position`.
-- `entry` holds the immutable PEP 691 file metadata; `yanked` and `yank_reason`
-- change in place.
CREATE TABLE IF NOT EXISTS pypi_files (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    filename TEXT COLLATE "C" NOT NULL,
    position BIGINT NOT NULL,
    yanked BIGINT NOT NULL,
    yank_reason TEXT COLLATE "C",
    entry TEXT COLLATE "C" NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, filename)
);
CREATE INDEX IF NOT EXISTS pypi_files_by_position
    ON pypi_files (namespace, ecosystem, name, position);

-- Distribution filenames permanently reserved after deletion from a Python
-- project, so a deleted filename can never be uploaded again.
CREATE TABLE IF NOT EXISTS removed_pypi_files (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    filename TEXT COLLATE "C" NOT NULL,
    generation BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, filename)
);

-- Manifests retained in an OCI image repository, keyed by content digest.
-- `entry` holds child digests and indexed referrer metadata.
CREATE TABLE IF NOT EXISTS oci_manifests (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    digest TEXT COLLATE "C" NOT NULL,
    entry TEXT COLLATE "C" NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, digest)
);

-- Mutable tags of an OCI image repository and the manifest digest each names.
CREATE TABLE IF NOT EXISTS oci_tags (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    tag TEXT COLLATE "C" NOT NULL,
    digest TEXT COLLATE "C" NOT NULL,
    updated BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, tag)
);

-- Insert-only: a version's artifact integrity. `accepted_generation` is the
-- namespace generation that accepted it, which is the id of the version's
-- initial publication.
CREATE TABLE IF NOT EXISTS versions (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    version TEXT COLLATE "C" NOT NULL,
    integrity TEXT COLLATE "C" NOT NULL,
    published_at BIGINT,
    accepted_generation BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version)
);

-- Insert-only: the manifest accepted for a version under an interpretation
-- profile. `inputs` is a JSON object of the digests of the raw inputs the
-- profile derived it from, such as the archive's package.json; it is empty
-- for the `submitted` profile, whose input is the manifest itself.
-- `accepted_digest` covers the manifest, README, profile, and inputs, which a
-- restatement of the version must match.
CREATE TABLE IF NOT EXISTS accepted_manifests (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    version TEXT COLLATE "C" NOT NULL,
    profile TEXT COLLATE "C" NOT NULL,
    manifest TEXT COLLATE "C" NOT NULL,
    inputs TEXT COLLATE "C" NOT NULL,
    accepted_digest TEXT COLLATE "C" NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version, profile)
);

-- Insert-only: the exact bytes of a version's release record, as accepted
-- under `profile` and served unchanged at its record route. `digest` is their
-- SHA-512, `sha512-<base64>`, which the record's log entry commits to, and
-- `log_index` the index of that entry in its namespace's log.
CREATE TABLE IF NOT EXISTS release_records (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    version TEXT COLLATE "C" NOT NULL,
    profile TEXT COLLATE "C" NOT NULL,
    record BYTEA NOT NULL,
    digest TEXT COLLATE "C" NOT NULL,
    log_index BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version, profile)
);

-- Insert-only: the transparency log of each namespace's records of one
-- ecosystem, one row per entry, sequenced in the transaction that accepts the
-- record. `idx` counts from 0 without gaps. Kept when a version or package is
-- removed: a log only grows.
CREATE TABLE IF NOT EXISTS log_entries (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    idx BIGINT NOT NULL,
    entry BYTEA NOT NULL,
    PRIMARY KEY (namespace, ecosystem, idx)
);

-- The tree each log's tiles and checkpoint were integrated to: its size and
-- its RFC 6962 root. Integration checks the stored tiles against it before it
-- extends a log, so altered tiles are never signed over, and only moves it to
-- a larger tree.
CREATE TABLE IF NOT EXISTS log_heads (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    size BIGINT NOT NULL,
    root BYTEA NOT NULL,
    PRIMARY KEY (namespace, ecosystem)
);

-- Kept out of the manifest so a README never inflates resolution metadata.
CREATE TABLE IF NOT EXISTS readmes (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    version TEXT COLLATE "C" NOT NULL,
    readme TEXT COLLATE "C",
    readme_filename TEXT COLLATE "C",
    PRIMARY KEY (namespace, ecosystem, name, version)
);

CREATE TABLE IF NOT EXISTS tags (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    tag TEXT COLLATE "C" NOT NULL,
    version TEXT COLLATE "C" NOT NULL,
    generation BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, tag)
);

-- Insert-only: every move of a tag, so a release-age or downgrade policy can
-- see when a tag moved, not only where it points. Kept when the package is
-- removed.
CREATE TABLE IF NOT EXISTS tag_history (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    tag TEXT COLLATE "C" NOT NULL,
    generation BIGINT NOT NULL,
    previous TEXT COLLATE "C",
    target TEXT COLLATE "C",
    principal TEXT COLLATE "C",
    changed_at BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, tag, generation)
);

-- Insert-only: the identity of every version removed from a package, by
-- unpublishing it or the whole package. Kept when the package is removed, so
-- a removed version is never accepted again and a version always names the
-- same bytes.
CREATE TABLE IF NOT EXISTS removed_versions (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    version TEXT COLLATE "C" NOT NULL,
    generation BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version)
);

-- Every manifest removed from an OCI repository, by deleting the manifest or
-- removing the repository, so a publication that introduced it is not replayed
-- after its removal. Kept when the repository is removed, with the generation
-- of its latest removal when pushed and removed again.
CREATE TABLE IF NOT EXISTS removed_oci_manifests (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    artifact TEXT COLLATE "C" NOT NULL,
    generation BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, artifact)
);

-- Mutable annotations of a version. `deprecated` is its JSON value.
-- `generation` is the generation that last changed the row.
CREATE TABLE IF NOT EXISTS annotations (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    version TEXT COLLATE "C" NOT NULL,
    deprecated TEXT COLLATE "C" NOT NULL,
    generation BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version)
);

-- A commit that introduced artifacts. `id` is the namespace generation it
-- committed. `name` is the publisher's, unique within the namespace.
CREATE TABLE IF NOT EXISTS publications (
    namespace TEXT COLLATE "C" NOT NULL,
    id BIGINT NOT NULL,
    name TEXT COLLATE "C",
    principal TEXT COLLATE "C",
    committed_at BIGINT NOT NULL,
    PRIMARY KEY (namespace, id)
);
CREATE UNIQUE INDEX IF NOT EXISTS publications_by_name
    ON publications (namespace, name) WHERE name IS NOT NULL;

-- Insert-only: each artifact belongs to every publication that introduced it.
-- Kept when the package is removed.
CREATE TABLE IF NOT EXISTS publication_members (
    namespace TEXT COLLATE "C" NOT NULL,
    publication BIGINT NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    version TEXT COLLATE "C" NOT NULL,
    artifact TEXT COLLATE "C" NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version, artifact, publication)
);
CREATE INDEX IF NOT EXISTS publication_members_by_publication
    ON publication_members (namespace, publication);

-- The outcome of a committed publication request, keyed by the principal
-- that sent it and its idempotency key, for the request's retries.
CREATE TABLE IF NOT EXISTS publication_outcomes (
    namespace TEXT COLLATE "C" NOT NULL,
    principal TEXT COLLATE "C" NOT NULL,
    idempotency_key TEXT COLLATE "C" NOT NULL,
    request_digest TEXT COLLATE "C" NOT NULL,
    generation BIGINT NOT NULL,
    publication BIGINT,
    committed_at BIGINT NOT NULL,
    PRIMARY KEY (namespace, principal, idempotency_key)
);
CREATE INDEX IF NOT EXISTS publication_outcomes_by_committed_at
    ON publication_outcomes (committed_at);

-- Release notes of a package version. `text` is NULL once deleted.
-- `initial_digest` is the digest of the text published with the version.
CREATE TABLE IF NOT EXISTS version_notes (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    version TEXT COLLATE "C" NOT NULL,
    text TEXT COLLATE "C",
    modified_at BIGINT NOT NULL,
    principal TEXT COLLATE "C",
    initial_digest TEXT COLLATE "C",
    PRIMARY KEY (namespace, ecosystem, name, version)
);

-- Release notes of a publication, as `version_notes` holds them for a version.
CREATE TABLE IF NOT EXISTS publication_notes (
    namespace TEXT COLLATE "C" NOT NULL,
    publication BIGINT NOT NULL,
    text TEXT COLLATE "C",
    modified_at BIGINT NOT NULL,
    principal TEXT COLLATE "C",
    initial_digest TEXT COLLATE "C",
    PRIMARY KEY (namespace, publication)
);

-- The bytes the hosted store holds, by identity (`<algorithm>-<hex>`), and
-- the incarnation stored under `<blob_id>.<incarnation>` now, `live`,
-- `reclaiming`, or `gone`. Never removed, so incarnations only grow and a key
-- a reclaim may remove is never written again. `changed_at` is when `state`
-- last changed.
CREATE TABLE IF NOT EXISTS blobs (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    blob_id TEXT COLLATE "C" NOT NULL,
    incarnation BIGINT NOT NULL,
    state TEXT COLLATE "C" NOT NULL,
    size BIGINT NOT NULL,
    changed_at BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, blob_id)
);
CREATE INDEX IF NOT EXISTS blobs_by_state ON blobs (namespace, ecosystem, state);

-- Each public file name of a package, mapped to the blob holding its bytes,
-- by the commit that publishes it or an image blob's upload, and dropped
-- with its version or package.
CREATE TABLE IF NOT EXISTS artifact_blobs (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    artifact TEXT COLLATE "C" NOT NULL,
    version TEXT COLLATE "C" NOT NULL,
    blob_id TEXT COLLATE "C" NOT NULL,
    mapped_at BIGINT NOT NULL,
    mapping TEXT COLLATE "C" NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, artifact)
);
CREATE INDEX IF NOT EXISTS artifact_blobs_by_blob_id
    ON artifact_blobs (namespace, ecosystem, blob_id);

-- The record of each archive a principal staged by the identity
-- (`<algorithm>-<hex>`) of its bytes ahead of the commit that claims it. The
-- bytes stay in the hosted store under `<blob_id>.<incarnation>`. `state` is
-- `incomplete`, `complete`, or `gone`; never removed, so incarnations only
-- grow and a key a sweep may remove is never written again. `staged_at` is
-- `0` once given back or claimed.
CREATE TABLE IF NOT EXISTS staged_archives (
    principal TEXT COLLATE "C" NOT NULL,
    blob_id TEXT COLLATE "C" NOT NULL,
    incarnation BIGINT NOT NULL,
    staged_at BIGINT NOT NULL,
    size BIGINT NOT NULL,
    state TEXT COLLATE "C" NOT NULL,
    PRIMARY KEY (principal, blob_id)
);
-- The records a principal still holds, so its usage and its sweeps read
-- them without the tombstones of every archive it ever staged.
CREATE INDEX IF NOT EXISTS staged_archives_held
    ON staged_archives (principal) WHERE state != 'gone';

-- Provisioned SCIM user accounts and deletion tombstones (`removed` = 1).
-- `deprovisions` counts transitions from active to inactive so replicas revoke
-- sessions even if an account is reactivated between polls.
CREATE TABLE IF NOT EXISTS scim_users (
    user_id TEXT COLLATE "C" NOT NULL PRIMARY KEY,
    active BIGINT NOT NULL,
    removed BIGINT NOT NULL,
    deprovisions BIGINT NOT NULL,
    attributes TEXT COLLATE "C" NOT NULL,
    generation BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS scim_users_by_generation
    ON scim_users (generation);

-- The hosted store this database is bound to.
CREATE TABLE IF NOT EXISTS hosted_store (
    singleton BIGINT PRIMARY KEY CHECK (singleton = 1),
    store_id TEXT COLLATE "C" NOT NULL
);

-- Every stored revision of the registry graph, the newest one current.
CREATE TABLE IF NOT EXISTS registry_graph_revisions (
    generation BIGINT NOT NULL PRIMARY KEY,
    document TEXT COLLATE "C" NOT NULL,
    actor_id TEXT COLLATE "C" NOT NULL,
    created_at BIGINT NOT NULL,
    restored_from BIGINT
);

-- Each hosted `(namespace, ecosystem)` a stored registry graph revision
-- stopped serving and no later revision serves again. Publishing into one is
-- refused.
CREATE TABLE IF NOT EXISTS retired_hosted_namespaces (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    PRIMARY KEY (namespace, ecosystem)
);

-- The active Cloudflare CDN cache-tag target shared by replicas of this database.
CREATE TABLE IF NOT EXISTS cdn_subscriptions (
    singleton BIGINT NOT NULL PRIMARY KEY CHECK (singleton = 1),
    target TEXT COLLATE "C" NOT NULL
);

-- Coalesced CDN cache invalidations (`package` = '*' for a full zone-tag purge,
-- else a package name).
CREATE TABLE IF NOT EXISTS cdn_purge_queue (
    package TEXT COLLATE "C" NOT NULL PRIMARY KEY,
    generation BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS cdn_purge_queue_by_generation
    ON cdn_purge_queue (generation);

CREATE TABLE IF NOT EXISTS staged_metadata (
    id TEXT COLLATE "C" PRIMARY KEY NOT NULL,
    generation BIGINT NOT NULL,
    body BYTEA NOT NULL
);

CREATE TABLE IF NOT EXISTS removed_packages (
    namespace TEXT COLLATE "C" NOT NULL,
    ecosystem TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    maintainers TEXT COLLATE "C" NOT NULL,
    unpublished_at BIGINT NOT NULL,
    unpublished_by TEXT COLLATE "C",
    PRIMARY KEY (namespace, ecosystem, name)
);

-- Generations stay unique after a record is removed and recreated.
CREATE TABLE IF NOT EXISTS record_generation (
    singleton BIGINT PRIMARY KEY CHECK (singleton = 1),
    generation BIGINT NOT NULL
);
INSERT INTO record_generation (singleton, generation) VALUES (1, 0)
    ON CONFLICT DO NOTHING;

-- Registered user accounts for SQL-backed auth.
CREATE TABLE IF NOT EXISTS users (
    id TEXT COLLATE "C" NOT NULL PRIMARY KEY,
    username TEXT COLLATE "C" NOT NULL UNIQUE,
    username_skeleton_hash TEXT COLLATE "C" UNIQUE,
    status TEXT COLLATE "C" NOT NULL,
    credential_kind TEXT COLLATE "C" NOT NULL,
    password_hash TEXT COLLATE "C",
    created_at BIGINT NOT NULL,
    name_revision BIGINT NOT NULL DEFAULT 0,
    status_revision BIGINT NOT NULL DEFAULT 0
);

-- Bearer tokens for SQL-backed and local SQLite auth.
CREATE TABLE IF NOT EXISTS tokens (
    token_hash TEXT COLLATE "C" NOT NULL PRIMARY KEY,
    user_id TEXT COLLATE "C" NOT NULL,
    created_at BIGINT NOT NULL,
    last_used_at BIGINT NOT NULL,
    readonly BIGINT NOT NULL DEFAULT 0,
    cidr_whitelist TEXT COLLATE "C" NOT NULL DEFAULT '[]',
    name TEXT COLLATE "C",
    expires_at BIGINT,
    kind TEXT COLLATE "C"
);
CREATE INDEX IF NOT EXISTS tokens_user_id ON tokens (user_id);
CREATE INDEX IF NOT EXISTS tokens_expires_at ON tokens (expires_at)
    WHERE expires_at IS NOT NULL;

-- Monotonic and reconciled counters for SQL-backed auth.
CREATE TABLE IF NOT EXISTS auth_counters (
    name TEXT COLLATE "C" NOT NULL PRIMARY KEY,
    value BIGINT NOT NULL
);
