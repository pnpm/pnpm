---
"pacquet": patch
---

On Linux, pnpm now resolves registry hostnames through the system resolver (`getaddrinfo`), as it already does on macOS and Windows and as pnpm 11 did. Previously, an `/etc/resolv.conf` containing an option the bundled pure-Rust resolver did not recognize, such as `options no_tld_query`, made pnpm ignore the configured nameservers and silently query Google's public DNS instead [#14469](https://github.com/pnpm/pnpm/issues/14469).
