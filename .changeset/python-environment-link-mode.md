---
"pacquet": minor
---

Python environments now use copy-on-write clones of wheel files when the filesystem supports them. Set `python.linkMode` to `copy`, `hardlink`, or `reflink` to choose how files are imported from the store. Reflink mode falls back to copies when cloning is unavailable. Hardlink mode shares file writes with the store and other environments.
