---
"pacquet": patch
---

Fixed the repeat-install fast path getting permanently stuck on the content-check branch when a manifest's modification time shared the same millisecond as the baseline (such as during a fast install or when files are copied with identical timestamps). The validation baseline is now post-dated by 1ms after a successful content check, ensuring that subsequent repeat installs correctly converge to the pure-mtime fast path.
