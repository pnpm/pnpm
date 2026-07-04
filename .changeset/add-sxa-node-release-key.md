---
"@pnpm/crypto.shasums-file": patch
"pnpm": patch
---

Added the Node.js release team's new signing key (Stewart X Addison, `655F3B5C1FB3FA8D1A0CA6BDE4A7D232B936D2FD`) to the embedded Node.js release keys, so runtimes whose `SHASUMS256.txt` is signed by the new releaser verify successfully.
