---
"pacquet": patch
---

`pnpm run` now exits after a `SIGTERM` in a container where pnpm is PID 1 and the script runs pnpm again, as `"start": "pnpm serve"` does. It used to keep waiting after the script had shut down, until the container runtime killed it.
