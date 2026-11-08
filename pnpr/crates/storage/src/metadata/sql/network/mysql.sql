-- The current generation of each hosted catalog.
CREATE TABLE IF NOT EXISTS catalogs (
    catalog VARBINARY(512) NOT NULL PRIMARY KEY,
    generation BIGINT NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- One row per package. `generation` is the catalog generation of the last
-- commit that changed it. `state` holds what no other table does: for npm,
-- the package-level fields of the packument; for any other ecosystem, its
-- whole document.
CREATE TABLE IF NOT EXISTS packages (
    catalog VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    generation BIGINT NOT NULL,
    state LONGBLOB NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Insert-only: a version's artifact integrity. `accepted_generation` is the
-- catalog generation that accepted it, which is the id of the version's
-- initial publication.
CREATE TABLE IF NOT EXISTS versions (
    catalog VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    revision_key VARBINARY(512) NOT NULL,
    integrity VARBINARY(512) NOT NULL,
    published_at BIGINT,
    accepted_generation BIGINT NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, version, revision_key)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Insert-only: the manifest accepted for a version under an interpretation
-- profile. `inputs` is a JSON object of the digests of the raw inputs the
-- profile derived it from, such as the archive's package.json; it is empty
-- for the `submitted` profile, whose input is the manifest itself.
-- `accepted_digest` covers the manifest, README, profile, and inputs, which a
-- restatement of the version must match.
CREATE TABLE IF NOT EXISTS accepted_manifests (
    catalog VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    revision_key VARBINARY(512) NOT NULL,
    profile VARBINARY(512) NOT NULL,
    manifest LONGTEXT NOT NULL,
    inputs LONGTEXT NOT NULL,
    accepted_digest LONGTEXT NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, version, revision_key, profile)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Insert-only: the exact bytes of a version's release record, as accepted
-- under `profile` and served unchanged at its record route. `digest` is their
-- SHA-512, `sha512-<base64>`, which the record's log entry commits to, and
-- `log_index` the index of that entry in its catalog's log.
CREATE TABLE IF NOT EXISTS release_records (
    catalog VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    revision_key VARBINARY(512) NOT NULL,
    profile VARBINARY(512) NOT NULL,
    record LONGBLOB NOT NULL,
    digest VARBINARY(512) NOT NULL,
    log_index BIGINT NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, version, revision_key, profile)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Insert-only: the transparency log of each catalog's records of one
-- ecosystem, one row per entry, sequenced in the transaction that accepts the
-- record. `idx` counts from 0 without gaps. Kept when a version or package is
-- removed: a log only grows.
CREATE TABLE IF NOT EXISTS log_entries (
    catalog VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    idx BIGINT NOT NULL,
    entry LONGBLOB NOT NULL,
    PRIMARY KEY (catalog, ecosystem, idx)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- The tree each log's tiles and checkpoint were integrated to: its size and
-- its RFC 6962 root. Integration checks the stored tiles against it before it
-- extends a log, so altered tiles are never signed over, and only moves it to
-- a larger tree.
CREATE TABLE IF NOT EXISTS log_heads (
    catalog VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    size BIGINT NOT NULL,
    root LONGBLOB NOT NULL,
    PRIMARY KEY (catalog, ecosystem)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Kept out of the manifest so a README never inflates resolution metadata.
CREATE TABLE IF NOT EXISTS readmes (
    catalog VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    revision_key VARBINARY(512) NOT NULL,
    readme LONGTEXT,
    readme_filename LONGTEXT,
    PRIMARY KEY (catalog, ecosystem, name, version, revision_key)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

CREATE TABLE IF NOT EXISTS tags (
    catalog VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    tag VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    generation BIGINT NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, tag)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Insert-only: every move of a tag, so a release-age or downgrade policy can
-- see when a tag moved, not only where it points. Kept when the package is
-- removed.
CREATE TABLE IF NOT EXISTS tag_history (
    catalog VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    tag VARBINARY(512) NOT NULL,
    generation BIGINT NOT NULL,
    previous LONGTEXT,
    target LONGTEXT,
    principal VARBINARY(512),
    changed_at BIGINT NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, tag, generation)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Insert-only: the identity of every version removed from a package, by
-- unpublishing it or the whole package. Kept when the package is removed, so
-- a removed version is never accepted again and a version and revision always
-- name the same bytes.
CREATE TABLE IF NOT EXISTS removed_versions (
    catalog VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    revision_key VARBINARY(512) NOT NULL,
    generation BIGINT NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, version, revision_key)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Mutable annotations of a version. `deprecated` is its JSON value.
-- `generation` is the generation that last changed the row.
CREATE TABLE IF NOT EXISTS annotations (
    catalog VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    deprecated LONGTEXT NOT NULL,
    generation BIGINT NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, version)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- A commit that introduced artifacts. `id` is the catalog generation it
-- committed. `name` is the publisher's, unique within the catalog.
CREATE TABLE IF NOT EXISTS publications (
    catalog VARBINARY(512) NOT NULL,
    id BIGINT NOT NULL,
    name VARBINARY(512),
    principal VARBINARY(512),
    committed_at BIGINT NOT NULL,
    PRIMARY KEY (catalog, id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
CREATE UNIQUE INDEX publications_by_name
    ON publications (catalog, name);

-- Insert-only: each artifact belongs to the publication that introduced it.
-- Kept when the package is removed.
CREATE TABLE IF NOT EXISTS publication_members (
    catalog VARBINARY(512) NOT NULL,
    publication BIGINT NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    artifact VARBINARY(512) NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, version, artifact)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
CREATE INDEX publication_members_by_publication
    ON publication_members (catalog, publication);

-- The outcome of a committed publication request, keyed by the principal
-- that sent it and its idempotency key, for the request's retries.
CREATE TABLE IF NOT EXISTS publication_outcomes (
    catalog VARBINARY(512) NOT NULL,
    principal VARBINARY(512) NOT NULL,
    idempotency_key VARBINARY(512) NOT NULL,
    request_digest LONGTEXT NOT NULL,
    generation BIGINT NOT NULL,
    publication BIGINT,
    committed_at BIGINT NOT NULL,
    PRIMARY KEY (catalog, principal, idempotency_key)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Release notes of a package version (`kind` = 'version', `subject` = the
-- version) or of a publication (`kind` = 'publication', `ecosystem` and
-- `name` empty, `subject` = its id). `text` is NULL once deleted.
-- `initial_digest` is the digest of the text published with the subject.
CREATE TABLE IF NOT EXISTS notes (
    catalog VARBINARY(512) NOT NULL,
    kind VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    subject VARBINARY(512) NOT NULL,
    text LONGTEXT,
    modified_at BIGINT NOT NULL,
    principal VARBINARY(512),
    initial_digest LONGTEXT,
    PRIMARY KEY (catalog, kind, ecosystem, name, subject)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- The bytes the hosted store holds, by identity (`<algorithm>-<hex>`), and
-- the incarnation stored under `<integrity>.<incarnation>` now, `live`,
-- `reclaiming`, or `gone`. Never removed, so incarnations only grow and a key
-- a reclaim may remove is never written again. `changed_at` is when `state`
-- last changed.
CREATE TABLE IF NOT EXISTS blobs (
    catalog VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    integrity VARBINARY(512) NOT NULL,
    incarnation BIGINT NOT NULL,
    state VARBINARY(512) NOT NULL,
    size BIGINT NOT NULL,
    changed_at BIGINT NOT NULL,
    PRIMARY KEY (catalog, ecosystem, integrity)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
CREATE INDEX blobs_by_state ON blobs (catalog, ecosystem, state);

-- Each public file name of a package, mapped to the blob holding its bytes,
-- by the commit that publishes it or an image blob's upload, and dropped
-- with its version or package.
CREATE TABLE IF NOT EXISTS artifact_blobs (
    catalog VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    artifact VARBINARY(512) NOT NULL,
    version VARBINARY(512) NOT NULL,
    integrity VARBINARY(512) NOT NULL,
    mapped_at BIGINT NOT NULL,
    mapping LONGTEXT NOT NULL,
    PRIMARY KEY (catalog, ecosystem, name, artifact)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
CREATE INDEX artifact_blobs_by_integrity
    ON artifact_blobs (catalog, ecosystem, integrity);

-- The record of each archive a principal staged by the hex SHA-512 digest
-- of its bytes ahead of the commit that claims it. The bytes stay in the
-- hosted store under `<digest>.<incarnation>`. `state` is `incomplete`,
-- `complete`, or `gone`; never removed, so incarnations only grow and a key
-- a sweep may remove is never written again. `staged_at` is `0` once given
-- back or claimed.
CREATE TABLE IF NOT EXISTS staged_archives (
    principal VARBINARY(512) NOT NULL,
    digest VARBINARY(512) NOT NULL,
    incarnation BIGINT NOT NULL,
    staged_at BIGINT NOT NULL,
    size BIGINT NOT NULL,
    state VARBINARY(512) NOT NULL,
    PRIMARY KEY (principal, digest)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
CREATE INDEX staged_archives_held
    ON staged_archives (principal);

CREATE TABLE IF NOT EXISTS registry_records (
    kind VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    generation BIGINT NOT NULL,
    body LONGBLOB NOT NULL,
    PRIMARY KEY (kind, name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

CREATE TABLE IF NOT EXISTS staged_metadata (
    id VARBINARY(512) PRIMARY KEY NOT NULL,
    generation BIGINT NOT NULL,
    body LONGBLOB NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

CREATE TABLE IF NOT EXISTS removed_packages (
    catalog VARBINARY(512) NOT NULL,
    ecosystem VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    maintainers LONGTEXT NOT NULL,
    unpublished_at BIGINT NOT NULL,
    unpublished_by LONGTEXT,
    PRIMARY KEY (catalog, ecosystem, name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;

-- Generations stay unique after a record is removed and recreated.
CREATE TABLE IF NOT EXISTS record_generation (
    singleton BIGINT PRIMARY KEY CHECK (singleton = 1),
    generation BIGINT NOT NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_as_cs;
INSERT INTO record_generation (singleton, generation) VALUES (1, 0)
    ON DUPLICATE KEY UPDATE singleton = record_generation.singleton;
