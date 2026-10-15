---
"@pnpm/pnpr": minor
---

pnpr now supports resolving client IP addresses through trusted reverse proxies using the new `trustedProxies` and `trustedProxyHeader` settings. Forwarding headers from trusted peers are traversed to record the client IP in access logs and enforce token CIDR restrictions [pnpm/tasks#131](https://github.com/pnpm/tasks/issues/131).
