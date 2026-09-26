---
"@pnpm/network.config": patch
---

`pickSettingByUrl` now returns a matching setting whose value is falsy, such as `false` or an empty string. It also matches a URL that has both a port and a query string against the settings of its path.
