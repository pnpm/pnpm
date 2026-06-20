---
"@pnpm/network.fetch": patch
"pnpm": patch
---

Fixed `pnpm` hanging (and crashing with an unhandled promise rejection) when a non-retryable network error such as `SELF_SIGNED_CERT_IN_CHAIN` occurs while fetching from a registry. The error is now rejected through the returned promise instead of being thrown inside the detached retry callback.
