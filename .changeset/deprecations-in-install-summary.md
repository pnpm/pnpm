---
"pacquet": minor
---

Deprecation warnings moved into the install summary. A deprecated dependency added by the install is marked `deprecated` on its summary line, with the version that is not deprecated when there is one, and gets no separate warning. Other deprecated direct dependencies and the deprecated subdependencies are listed right after the summary instead of during resolution.
