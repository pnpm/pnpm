---
"pacquet": minor
---

The experimental loaded linker now uses temporary directories for approved builds when the side-effects cache is enabled. Compatible build outputs load directly from the store. Packages with filesystem imports or package-location access in their JavaScript sources are materialized automatically.
