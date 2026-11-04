---
"pacquet": minor
---

`pnpm pack --json` prints the `integrity` and `size` of every archive it writes, including each platform variant's archive. Publishing from several machines can then reference an archive by its integrity without hashing it again.
