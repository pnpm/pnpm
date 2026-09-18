---
"pacquet": minor
---

`pnpm add` accepts a [Package URL](https://github.com/package-url/purl-spec) in place of a package name. `pnpm add pkg:npm/express@4.18.2` saves `express` to `package.json`, `pnpm add pkg:cargo/serde@1.0.188` saves `serde` to `Cargo.toml`, and `pnpm add pkg:pypi/requests@2.31.0` saves `requests` to `pyproject.toml`. A Package URL names one release, so a Cargo or PyPI Package URL saves an exact requirement.
