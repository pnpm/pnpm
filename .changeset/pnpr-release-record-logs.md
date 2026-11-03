---
"@pnpm/pnpr": minor
---

pnpr logs the release records of the npm versions it hosts with SQLite or libSQL metadata. Each hosted organization has a transparency log per ecosystem, in the C2SP tiled log format. A version's log entry is recorded in the same transaction that accepts the version. With `releaseRecords.logSigningKey` set, pnpr then adds the entry to the log's tiles and signs a new checkpoint, keeping both in the hosted store. The metadata database's schema changed, so a database written by an earlier pnpr is refused.
