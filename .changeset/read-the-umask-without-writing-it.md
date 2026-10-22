---
"pacquet": patch
---

Reading the file-creation mask on Linux no longer sets it. pnpm had to replace the mask to learn what it was, and a file another thread created in the window between the two calls was created unmasked.
