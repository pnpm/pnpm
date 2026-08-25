---
"pacquet": patch
---

pnpm now decodes package archives using a bounded amount of memory, whatever an archive or a registry claims about its size. A gzipped tarball that inflates past what a whole-archive decode may hold is extracted as a stream instead, a response body that keeps arriving is extracted while it downloads rather than accumulated in full, and a zip entry is read only as far as the size its archive records. No archive is refused for being large — everything that installed before still installs.
