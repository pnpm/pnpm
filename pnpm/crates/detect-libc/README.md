# Attribution

This crate is a Rust port of [detect-libc](https://github.com/lovell/detect-libc)
originally authored by Lovell Fuller and others (Copyright 2017).

The original project is licensed under the Apache License, Version 2.0.

## Changes from the original
- Ported from JavaScript/Node.js to Rust
- Reduced API to what is necessary for `pacquet`'s use case (`family` function renamed to `detect`)
