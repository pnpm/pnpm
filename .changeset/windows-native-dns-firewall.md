---
"pacquet": patch
---

On Windows, pnpm now resolves host names through the system resolver instead of its own DNS client. The built-in client bound a UDP socket for every lookup, which made Windows Defender Firewall ask to allow `pnpm.exe` again after every `pnpm self-update` [#14405](https://github.com/pnpm/pnpm/issues/14405).
