---
"pacquet": minor
---

Added experimental `nodeLinker: { type: loaded }` installation. Compatible dependencies load directly from the content-addressable store through an automatically registered Node.js loader. `nodeLinker.excluded` selects packages and their dependency trees to install in the global virtual store.
