---
"@pnpm/pnpr": minor
---

pnpr can now publish packages of more than one ecosystem in a single transaction. `PUT /-/pnpr/v0/publish` takes a batch whose entries each name their `ecosystem`, so a workspace that ships an npm package, a crate and a Python distribution releases them together. A batch that fails a check publishes none of it. A server that stops midway finishes the release on the next startup. An entry without an `ecosystem` is an npm publish document, the same one `PUT /-/pnpm/v1/publish` takes.
