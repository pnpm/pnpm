---
"pacquet": patch
---

A `link:` or relative-path dependency of a local directory dependency now resolves from that directory. In any other package, such as a local tarball, it resolves from the lockfile directory. Both used to resolve from the importing project's directory [pnpm/tasks#319](https://github.com/pnpm/tasks/issues/319).

Two workspace projects that declare the same injected relative-path dependency, such as `../child`, no longer share the directory the first one resolved.
