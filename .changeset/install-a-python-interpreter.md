---
"pacquet": minor
---

`pnpm install` now installs a Python interpreter when no interpreter on the machine fits the project [#14945](https://github.com/pnpm/pnpm/issues/14945). The builds are [python-build-standalone](https://github.com/astral-sh/python-build-standalone)'s, the ones uv, rye and hatch install too, checked against the digest their release publishes. One interpreter is shared by every project on the machine, and later installs find it without downloading anything. Set `python.downloads` to `never` to keep pnpm from installing any, and `python.downloadUrl` to install them from a mirror.
