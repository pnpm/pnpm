---
"@pnpm/pnpr": minor
---

With SQLite metadata, pnpr stores release notes for package versions and publications. A publish can include them in a `notes` field. Publishers can edit or delete a note later. npm clients read a version's note at `/npm/<name>/<version>/notes`. A package's notes are listed at `/npm/<name>/-/notes`. A publication's note is at `/-/publications/<org>/<id-or-name>/notes`.
