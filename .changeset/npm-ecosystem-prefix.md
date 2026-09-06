---
"@pnpm/pnpr": major
---

An npm client's registry URL is now `https://<pnpr>/npm/`, or `https://<pnpr>/npm/~<name>/` for a named registry. The host's root no longer serves packages. `/-/ping`, the `/-/pnpr/v0/` protocols and the account endpoints stay at the root. The npm packages named `npm`, `cargo` and `pypi` can now be installed. Their names collided with the ecosystem prefixes.
