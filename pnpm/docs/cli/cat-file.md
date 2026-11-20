---
id: cat-file
title: "pnpm cat-file"
---

Prints the contents of a file from the store, given its `sha512-<base64>` hash. The hex `digest` that [`pnpm cat-index`](./cat-index.md) prints is not accepted. For example:

```sh
pnpm cat-file sha512-mvavhfVcEREI7d8dfvfvIkuBLnx7+rrkHHnPi8mpEDUlNpY4CUY+CvJ5mrrLl18iQYo1odFwBV7z/cOypG7xxQ==
```
