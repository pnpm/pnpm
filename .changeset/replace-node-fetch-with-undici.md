---
"@pnpm/network.fetch": major
"@pnpm/fetching.types": major
"pnpm": minor
---

Replace node-fetch with undici as the HTTP client [#10537](https://github.com/pnpm/pnpm/pull/10537).

- Use undici's native `fetch()` with dispatcher-based connection management
- Support HTTP, HTTPS, SOCKS4, and SOCKS5 proxies
- Cache dispatchers via LRU cache keyed by connection parameters
- Handle per-registry client certificates via nerf-dart URL matching
- Convert test HTTP mocking from nock to undici MockAgent
