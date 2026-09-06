---
"@pnpm/pnpr": patch
---

A `cargo publish` or `twine upload` that pnpr accepted is now recorded in one crash-safe step, as an npm publish already was. If the server stops between storing the file and recording it in the crate or project document, the next startup completes the publish instead of leaving a file nothing points at.
