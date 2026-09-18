---
"pacquet": minor
---

pnpm can now resolve `pylock.toml` for several platforms and Python versions at once. `supportedArchitectures` names the platforms to lock for and `python.versions` the versions. Every platform is paired with every version. One committed lockfile then serves Linux CI and macOS or Windows contributors [#14945](https://github.com/pnpm/pnpm/issues/14945).

```yaml
supportedArchitectures:
  - linux-x64-manylinux_2_28
  - darwin-arm64
  - win32-x64
python:
  enabled: true
  versions: ['3.12', '3.13']
```

The lockfile pins the wheel each environment takes for a distribution. It marks a package only some environments install. `pnpm install` takes the packages and wheels of the environment its interpreter matches, and refuses an interpreter none of them stand for. pnpm resolves a project that declares environments itself, not through the server `pnprServer` names. Naming neither setting locks for the interpreter running the install.
