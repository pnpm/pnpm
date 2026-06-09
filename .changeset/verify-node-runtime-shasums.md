---
"@pnpm/crypto.shasums-file": patch
"@pnpm/node.fetcher": patch
"@pnpm/plugin-commands-env": patch
"pnpm": patch
---

pnpm now verifies the detached OpenPGP signature of a Node.js release's `SHASUMS256.txt` against the Node.js release team's public keys (embedded in the pnpm CLI) before trusting its hashes. The Node.js download mirror is repository-configurable (`node-mirror:<channel>` in `.npmrc`), and the integrity check previously trusted a `SHASUMS256.txt` fetched from that same mirror — a circular check that a malicious mirror could satisfy with a tampered binary and matching hashes. A mirror that proxies the real signed SHASUMS keeps working unchanged. Only the `release` channel publishes signed SHASUMS files, so pre-release channels (rc, nightly, …) remain unverified.
