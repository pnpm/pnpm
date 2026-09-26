---
"pacquet": patch
---

`proxy=false` now turns proxying off even when `HTTP_PROXY`, `HTTPS_PROXY`, or `ALL_PROXY` is set. pnpm no longer sends requests through a proxy named only in `ALL_PROXY`.
