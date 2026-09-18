---
"pacquet": patch
---

Python interpreter installation now retries historical release metadata requests. It caches the release list for up to 24 hours and refreshes it once after a lookup miss. When a release omits the current platform, the search samples at most eight other releases before reporting that the lookup is inconclusive.
