---
"pacquet": patch
---

Fixed `catalog:` references failing to resolve when installing through a pnpr server, which errored with "No catalog entry '<name>' was found for catalog 'default'." even though the catalog entry existed. The workspace the server reconstructs from the request has no catalog sections, so the client now sends its catalogs along with the request [#13232](https://github.com/pnpm/pnpm/issues/13232).
