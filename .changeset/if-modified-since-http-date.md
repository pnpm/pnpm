---
"pacquet": patch
---

Conditional metadata requests send `If-Modified-Since` as an HTTP-date instead of the mirror's raw ISO-8601 `modified` value, so registries can answer `304 Not Modified` instead of re-serving the full packument [#13104](https://github.com/pnpm/pnpm/issues/13104).
