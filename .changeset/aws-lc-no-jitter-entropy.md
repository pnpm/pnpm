---
"pacquet": patch
"@pnpm/pnpr": patch
---

Fixed `pnpm` and `pnpr` occasionally crashing on the first HTTPS request, mostly on Windows virtual machines under load. They now seed TLS randomness from the operating system's random number generator [pnpm/tasks#239](https://github.com/pnpm/tasks/issues/239).
