---
"pacquet": minor
---

pnpm can now resolve `pylock.toml` for several platforms and Python versions at once. `python.platforms` and `python.pythonVersions` name the environments to lock for. Every platform is paired with every version. One committed lockfile then serves Linux CI and macOS or Windows contributors [#14945](https://github.com/pnpm/pnpm/issues/14945).

```yaml
python:
  enabled: true
  platforms:
    - x86_64-manylinux_2_28
    - aarch64-apple-darwin
    - x86_64-pc-windows-msvc
  pythonVersions: ['3.12', '3.13']
```

The lockfile pins the wheel each environment takes for a distribution. It marks a package only some environments install. `pnpm install` takes the packages and wheels of the environment its interpreter matches, and refuses an interpreter none of them stand for. pnpm resolves a project that declares environments itself, not through the server `pnprServer` names. Declaring neither setting locks for the interpreter running the install.
