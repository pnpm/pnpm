# pnpm v12 (pacquet)

The [pnpm](https://pnpm.io) v12 CLI is implemented in Rust. `pacquet` is its
in-repository package name; the published CLI and executable are named `pnpm`.

pnpm v12 is the target for new feature development. The TypeScript pnpm v11
CLI under `../pnpm11/` is maintained for bug fixes. Bugs present in both
versions are fixed in both implementations. Bugs present in only one version
are fixed in that version. New features are not backported to v11.

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for development setup, debugging, testing, and benchmarking.

## Python registry routing (experimental)

Python registry entries declare which package names they serve. pnpm selects
one registry before requesting a package; the order of entries has no effect.

```yaml
registries:
  https://internal.example/simple/:
    ecosystem: pypi
    packages:
      - "company-*"
      - "legacy-internal-package"
  https://download.pytorch.org/whl/cpu/:
    ecosystem: pypi
    packages:
      - "torch"
  https://pypi.org/simple/:
    ecosystem: pypi
    packages:
      - "**"
```

`packages` accepts exact distribution names, a name prefix followed by `*`,
and `**` for the default index. Names use Python's normalization: case is
ignored, and runs of `.`, `_`, and `-` become `-`. Other glob forms are rejected.

Registry dependencies matching a name or prefix resolve exclusively from that
registry, including transitive dependencies and isolated build dependencies. A missing
package, incompatible version, or registry error is final. pnpm does not retry
another registry. Overlapping names or prefixes assigned to different registries
and multiple default indexes are configuration errors.

Omitting `packages` declares a default index too, so a single custom index needs
no patterns. With no Python registries configured, pnpm uses PyPI. Once Python
registries are configured, packages outside their claims require a declared
default index. PyPI is not added implicitly.

Package routing changes invalidate Python lockfiles. Credentials remain in
`.npmrc` and must not appear in `registries`.

## Benchmark

![](https://pnpm.io/img/benchmarks/alotta-files-pnpm.svg)
