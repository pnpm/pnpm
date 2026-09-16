---
"pacquet": minor
---

`pylock.toml` can now be resolved for several platforms and Python versions at once, so one committed lockfile serves Linux CI and macOS or Windows contributors. `python.platforms` and `python.pythonVersions` name the environments to lock for, and every platform is paired with every version [#14945](https://github.com/pnpm/pnpm/issues/14945).

```yaml
python:
  enabled: true
  platforms:
    - x86_64-manylinux_2_28
    - aarch64-apple-darwin
    - x86_64-pc-windows-msvc
  pythonVersions: ['3.12', '3.13']
```

The lockfile pins the wheel each environment takes for a distribution, and marks a package only some of them install. `pnpm install` takes the packages and wheels of the environment its interpreter matches, and refuses an interpreter none of them stand for. Declaring neither setting locks for the interpreter running the install, as before.
