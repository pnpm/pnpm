-- The current generation of each hosted namespace.
CREATE TABLE IF NOT EXISTS namespaces (
    namespace VARBINARY(512) NOT NULL PRIMARY KEY,
    generation BIGINT NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- One row per package. `generation` is the namespace generation of the last
-- commit that changed it. `state_version` is the format version of `state`,
-- which holds the package-level fields no child table stores.
CREATE TABLE IF NOT EXISTS packages (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    generation BIGINT NOT NULL,
    state_version BIGINT NOT NULL,
    state LONGBLOB NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Published versions of a Cargo crate, ordered by `position` as the sparse
-- index serves them. `entry` holds the immutable sparse-index fields; `yanked`
-- flips in place.
CREATE TABLE IF NOT EXISTS cargo_versions (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    position BIGINT NOT NULL,
    yanked BIGINT NOT NULL,
    entry LONGTEXT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
CREATE INDEX cargo_versions_by_position
    ON cargo_versions (namespace, ecosystem, name, position);

-- Published distribution files of a Python project, ordered by `position`.
-- `entry` holds the immutable PEP 691 file metadata; `yanked` and `yank_reason`
-- change in place.
CREATE TABLE IF NOT EXISTS pypi_files (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    filename VARBINARY(512) NOT NULL,
    position BIGINT NOT NULL,
    yanked BIGINT NOT NULL,
    yank_reason LONGTEXT,
    entry LONGTEXT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, filename)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
CREATE INDEX pypi_files_by_position
    ON pypi_files (namespace, ecosystem, name, position);

-- Distribution filenames permanently reserved after deletion from a Python
-- project, so a deleted filename can never be uploaded again.
CREATE TABLE IF NOT EXISTS removed_pypi_files (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    filename VARBINARY(512) NOT NULL,
    generation BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, filename)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Manifests retained in an OCI image repository, keyed by content digest.
-- `entry` holds child digests and indexed referrer metadata.
CREATE TABLE IF NOT EXISTS oci_manifests (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    digest VARBINARY(512) NOT NULL,
    entry LONGTEXT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, digest)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Mutable tags of an OCI image repository and the manifest digest each names.
CREATE TABLE IF NOT EXISTS oci_tags (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    tag VARBINARY(512) NOT NULL,
    digest VARBINARY(512) NOT NULL,
    updated BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, tag)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Insert-only: a version's artifact integrity. `accepted_generation` is the
-- namespace generation that accepted it, which is the id of the version's
-- initial publication.
CREATE TABLE IF NOT EXISTS versions (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    integrity VARBINARY(512) NOT NULL,
    published_at BIGINT,
    accepted_generation BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Insert-only: the manifest accepted for a version under an interpretation
-- profile. `inputs` is a JSON object of the digests of the raw inputs the
-- profile derived it from, such as the archive's package.json; it is empty
-- for the `submitted` profile, whose input is the manifest itself.
-- `accepted_digest` covers the manifest, README, profile, and inputs, which a
-- restatement of the version must match.
CREATE TABLE IF NOT EXISTS accepted_manifests (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    profile VARBINARY(512) NOT NULL,
    manifest LONGTEXT NOT NULL,
    inputs LONGTEXT NOT NULL,
    accepted_digest LONGTEXT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version, profile)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Insert-only: the exact bytes of a version's release record, as accepted
-- under `profile` and served unchanged at its record route. `digest` is their
-- SHA-512, `sha512-<base64>`, which the record's log entry commits to, and
-- `log_index` the index of that entry in its namespace's log.
CREATE TABLE IF NOT EXISTS release_records (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    profile VARBINARY(512) NOT NULL,
    record LONGBLOB NOT NULL,
    digest VARBINARY(512) NOT NULL,
    log_index BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version, profile)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Insert-only: the transparency log of each namespace's records of one
-- ecosystem, one row per entry, sequenced in the transaction that accepts the
-- record. `idx` counts from 0 without gaps. Kept when a version or package is
-- removed: a log only grows.
CREATE TABLE IF NOT EXISTS log_entries (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    idx BIGINT NOT NULL,
    entry LONGBLOB NOT NULL,
    PRIMARY KEY (namespace, ecosystem, idx)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- The tree each log's tiles and checkpoint were integrated to: its size and
-- its RFC 6962 root. Integration checks the stored tiles against it before it
-- extends a log, so altered tiles are never signed over, and only moves it to
-- a larger tree.
CREATE TABLE IF NOT EXISTS log_heads (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    size BIGINT NOT NULL,
    root LONGBLOB NOT NULL,
    PRIMARY KEY (namespace, ecosystem)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Kept out of the manifest so a README never inflates resolution metadata.
CREATE TABLE IF NOT EXISTS readmes (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    readme LONGTEXT,
    readme_filename LONGTEXT,
    PRIMARY KEY (namespace, ecosystem, name, version)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

CREATE TABLE IF NOT EXISTS tags (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    tag VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    generation BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, tag)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Insert-only: every move of a tag, so a release-age or downgrade policy can
-- see when a tag moved, not only where it points. Kept when the package is
-- removed.
CREATE TABLE IF NOT EXISTS tag_history (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    tag VARBINARY(512) NOT NULL,
    generation BIGINT NOT NULL,
    previous LONGTEXT,
    target LONGTEXT,
    principal VARBINARY(512),
    changed_at BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, tag, generation)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Insert-only: the identity of every version removed from a package, by
-- unpublishing it or the whole package. Kept when the package is removed, so
-- a removed version is never accepted again and a version always names the
-- same bytes.
CREATE TABLE IF NOT EXISTS removed_versions (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    generation BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Every manifest removed from an OCI repository, by deleting the manifest or
-- removing the repository, so a publication that introduced it is not replayed
-- after its removal. Kept when the repository is removed, with the generation
-- of its latest removal when pushed and removed again.
CREATE TABLE IF NOT EXISTS removed_oci_manifests (
    namespace VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    artifact VARBINARY(512) NOT NULL,
    generation BIGINT NOT NULL,
    PRIMARY KEY (namespace, name, artifact)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Mutable annotations of a version. `deprecated` is its JSON value.
-- `generation` is the generation that last changed the row.
CREATE TABLE IF NOT EXISTS annotations (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    deprecated LONGTEXT NOT NULL,
    generation BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- A commit that introduced artifacts. `id` is the namespace generation it
-- committed. `name` is the publisher's, unique within the namespace.
CREATE TABLE IF NOT EXISTS publications (
    namespace VARBINARY(512) NOT NULL,
    id BIGINT NOT NULL,
    name VARBINARY(512),
    principal VARBINARY(512),
    committed_at BIGINT NOT NULL,
    PRIMARY KEY (namespace, id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
CREATE UNIQUE INDEX publications_by_name
    ON publications (namespace, name);

-- Insert-only: each artifact belongs to every publication that introduced it.
-- Kept when the package is removed.
CREATE TABLE IF NOT EXISTS publication_members (
    namespace VARBINARY(512) NOT NULL,
    publication BIGINT NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    artifact VARBINARY(512) NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, version, artifact, publication)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
CREATE INDEX publication_members_by_publication
    ON publication_members (namespace, publication);

-- The outcome of a committed publication request, keyed by the principal
-- that sent it and its idempotency key, for the request's retries.
CREATE TABLE IF NOT EXISTS publication_outcomes (
    namespace VARBINARY(512) NOT NULL,
    principal VARBINARY(512) NOT NULL,
    idempotency_key VARBINARY(512) NOT NULL,
    request_digest LONGTEXT NOT NULL,
    generation BIGINT NOT NULL,
    publication BIGINT,
    committed_at BIGINT NOT NULL,
    PRIMARY KEY (namespace, principal, idempotency_key)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
CREATE INDEX publication_outcomes_by_committed_at
    ON publication_outcomes (committed_at);

-- Release notes of a package version (`kind` = 'version', `subject` = the
-- version) or of a publication (`kind` = 'publication', `ecosystem` and
-- `name` empty, `subject` = its id). `text` is NULL once deleted.
-- `initial_digest` is the digest of the text published with the subject.
CREATE TABLE IF NOT EXISTS notes (
    namespace VARBINARY(512) NOT NULL,
    kind VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    subject VARBINARY(512) NOT NULL,
    text LONGTEXT,
    modified_at BIGINT NOT NULL,
    principal VARBINARY(512),
    initial_digest LONGTEXT,
    PRIMARY KEY (namespace, kind, ecosystem, name, subject)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- The bytes the hosted store holds, by identity (`<algorithm>-<hex>`), and
-- the incarnation stored under `<blob_id>.<incarnation>` now, `live`,
-- `reclaiming`, or `gone`. Never removed, so incarnations only grow and a key
-- a reclaim may remove is never written again. `changed_at` is when `state`
-- last changed.
CREATE TABLE IF NOT EXISTS blobs (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    blob_id VARBINARY(512) NOT NULL,
    incarnation BIGINT NOT NULL,
    state VARBINARY(512) NOT NULL,
    size BIGINT NOT NULL,
    changed_at BIGINT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, blob_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
CREATE INDEX blobs_by_state ON blobs (namespace, ecosystem, state);

-- Each public file name of a package, mapped to the blob holding its bytes,
-- by the commit that publishes it or an image blob's upload, and dropped
-- with its version or package.
CREATE TABLE IF NOT EXISTS artifact_blobs (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    artifact VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    blob_id VARBINARY(512) NOT NULL,
    mapped_at BIGINT NOT NULL,
    mapping LONGTEXT NOT NULL,
    PRIMARY KEY (namespace, ecosystem, name, artifact)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
CREATE INDEX artifact_blobs_by_blob_id
    ON artifact_blobs (namespace, ecosystem, blob_id);

-- The record of each archive a principal staged by the identity
-- (`<algorithm>-<hex>`) of its bytes ahead of the commit that claims it. The
-- bytes stay in the hosted store under `<blob_id>.<incarnation>`. `state` is
-- `incomplete`, `complete`, or `gone`; never removed, so incarnations only
-- grow and a key a sweep may remove is never written again. `staged_at` is
-- `0` once given back or claimed.
CREATE TABLE IF NOT EXISTS staged_archives (
    principal VARBINARY(512) NOT NULL,
    blob_id VARBINARY(512) NOT NULL,
    incarnation BIGINT NOT NULL,
    staged_at BIGINT NOT NULL,
    size BIGINT NOT NULL,
    state VARBINARY(512) NOT NULL,
    PRIMARY KEY (principal, blob_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
CREATE INDEX staged_archives_held
    ON staged_archives (principal);

-- Provisioned SCIM user accounts and deletion tombstones (`removed` = 1).
-- `deprovisions` counts transitions from active to inactive so replicas revoke
-- sessions even if an account is reactivated between polls.
CREATE TABLE IF NOT EXISTS scim_users (
    user_id VARBINARY(512) NOT NULL PRIMARY KEY,
    active BIGINT NOT NULL,
    removed BIGINT NOT NULL,
    deprovisions BIGINT NOT NULL,
    attributes LONGTEXT NOT NULL,
    generation BIGINT NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
CREATE INDEX scim_users_by_generation
    ON scim_users (generation);

-- Registries whose team roster has been initialized in the database.
CREATE TABLE IF NOT EXISTS team_rosters (
    registry VARBINARY(512) NOT NULL PRIMARY KEY,
    generation BIGINT NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

CREATE TABLE IF NOT EXISTS teams (
    registry VARBINARY(512) NOT NULL,
    team_id VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    position BIGINT NOT NULL,
    PRIMARY KEY (registry, team_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
CREATE UNIQUE INDEX teams_by_name
    ON teams (registry, name);
CREATE INDEX teams_by_position
    ON teams (registry, position);

CREATE TABLE IF NOT EXISTS team_members (
    registry VARBINARY(512) NOT NULL,
    team_id VARBINARY(512) NOT NULL,
    user_id VARBINARY(512) NOT NULL,
    PRIMARY KEY (registry, team_id, user_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Registries with stored API rule overrides, and the generation of their
-- current override set.
CREATE TABLE IF NOT EXISTS rule_override_sets (
    registry VARBINARY(512) NOT NULL PRIMARY KEY,
    generation BIGINT NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Overridden rule lists of a registry (`pattern` = '' for the registry-level
-- default `access`, else a declared package pattern). Presence with zero rows
-- in `rule_override_grants` represents an empty access list (`[]`).
CREATE TABLE IF NOT EXISTS rule_overrides (
    registry VARBINARY(512) NOT NULL,
    pattern VARBINARY(512) NOT NULL,
    action VARBINARY(512) NOT NULL,
    PRIMARY KEY (registry, pattern, action)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

CREATE TABLE IF NOT EXISTS rule_override_grants (
    registry VARBINARY(512) NOT NULL,
    pattern VARBINARY(512) NOT NULL,
    action VARBINARY(512) NOT NULL,
    position BIGINT NOT NULL,
    principal LONGTEXT NOT NULL,
    PRIMARY KEY (registry, pattern, action, position)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- The active Cloudflare CDN cache-tag target shared by replicas of this database.
CREATE TABLE IF NOT EXISTS cdn_subscriptions (
    singleton BIGINT NOT NULL PRIMARY KEY CHECK (singleton = 1),
    target LONGTEXT NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Coalesced CDN cache invalidations (`package` = '*' for a full zone-tag purge,
-- else a package name).
CREATE TABLE IF NOT EXISTS cdn_purge_queue (
    package VARBINARY(512) NOT NULL PRIMARY KEY,
    generation BIGINT NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
CREATE INDEX cdn_purge_queue_by_generation
    ON cdn_purge_queue (generation);

CREATE TABLE IF NOT EXISTS staged_metadata (
    id VARBINARY(512) PRIMARY KEY NOT NULL,
    generation BIGINT NOT NULL,
    body LONGBLOB NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

CREATE TABLE IF NOT EXISTS removed_packages (
    namespace VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    maintainers LONGTEXT NOT NULL,
    unpublished_at BIGINT NOT NULL,
    unpublished_by LONGTEXT,
    PRIMARY KEY (namespace, ecosystem, name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Generations stay unique after a record is removed and recreated.
CREATE TABLE IF NOT EXISTS record_generation (
    singleton BIGINT PRIMARY KEY CHECK (singleton = 1),
    generation BIGINT NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
INSERT INTO record_generation (singleton, generation) VALUES (1, 0)
    ON DUPLICATE KEY UPDATE singleton = record_generation.singleton;

-- Registered user accounts for SQL-backed auth.
CREATE TABLE IF NOT EXISTS users (
    id VARCHAR(512) NOT NULL PRIMARY KEY,
    username VARCHAR(512) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL UNIQUE,
    username_skeleton_hash VARCHAR(512) UNIQUE,
    status VARCHAR(512) NOT NULL,
    credential_kind VARCHAR(512) NOT NULL,
    password_hash LONGTEXT,
    created_at BIGINT NOT NULL,
    name_revision BIGINT NOT NULL DEFAULT 0
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Bearer tokens for SQL-backed and local SQLite auth.
CREATE TABLE IF NOT EXISTS tokens (
    token_hash VARCHAR(512) NOT NULL PRIMARY KEY,
    user_id VARCHAR(512) NOT NULL,
    created_at BIGINT NOT NULL,
    last_used_at BIGINT NOT NULL,
    readonly BIGINT NOT NULL DEFAULT 0,
    cidr_whitelist VARCHAR(4096) NOT NULL DEFAULT '[]',
    name VARCHAR(512),
    expires_at BIGINT,
    kind VARCHAR(512)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
CREATE INDEX tokens_user_id ON tokens (user_id);
CREATE INDEX tokens_expires_at ON tokens (expires_at);

-- Monotonic and reconciled counters for SQL-backed auth.
CREATE TABLE IF NOT EXISTS auth_counters (
    name VARCHAR(512) NOT NULL PRIMARY KEY,
    value BIGINT NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
