---
"@pnpm/crypto.shasums-file": minor
"@pnpm/engine.runtime.node-resolver": patch
"pnpm": patch
---

Security: pnpm now verifies the OpenPGP signature of a downloaded Node.js runtime's `SHASUMS256.txt` before trusting its integrity hashes.

When a repository requests a Node.js runtime (e.g. via `devEngines.runtime` / `useNodeVersion`), the download mirror is repository-configurable through `node-mirror:<channel>`. The integrity of the downloaded binary was only checked against `SHASUMS256.txt` fetched from that same mirror — a circular check that a malicious mirror could satisfy by serving a tampered binary together with a matching `SHASUMS256.txt`. pnpm then executes the binary (for example to run lifecycle scripts).

pnpm now fetches `SHASUMS256.txt.sig` and verifies the detached OpenPGP signature against the Node.js release team's public keys, which ship embedded in the pnpm CLI. A mirror that serves a tampered binary cannot also produce a valid signature, so the download fails to verify. The embedded keys are kept current by a release-time check against the canonical `nodejs/release-keys` list.

The musl variants from the hardcoded `unofficial-builds.nodejs.org` mirror are not repository-configurable and are signed by a different key, so they continue to be trusted over TLS.
