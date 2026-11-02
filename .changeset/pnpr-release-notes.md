---
"@pnpm/pnpr": minor
---

With SQLite metadata, pnpr stores release notes for package versions and publications. A publish can send them in a `notes` field, and they can be edited later. npm clients read them at `/npm/<name>/<version>/notes` and `/npm/<name>/-/notes`, and a publication's note is at `/-/publications/<org>/<id-or-name>/notes`.
