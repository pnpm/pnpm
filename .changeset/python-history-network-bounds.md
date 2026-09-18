---
"pacquet": patch
---

Python interpreter installation now retries historical release metadata requests. It caches the release list for up to 24 hours and refreshes it once after a lookup miss. Searches across releases that omit the current platform now limit the number of neighboring releases they probe.
