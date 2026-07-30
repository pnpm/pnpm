## 12.0.0-beta.1

### Minor Changes

- Made peer resolution significantly faster in large multi-importer workspaces (a 114-importer workspace's resolution dropped from ~77s to ~36s): importers whose hoist rounds converged no longer re-walk their dependency forest every round, later rounds walk only newly added direct dependencies, ownership handovers with an unchanged peer context no longer invalidate shared walk caches, and the resolver's internal hash maps use a faster hash. Peer dependencies provided by multiple candidate versions may resolve to a different (still range-valid) provider than before, which can shift some peer-variant suffixes in `pnpm-lock.yaml` once.

- `pnpm login` no longer requires an interactive terminal when the registry supports web-based login: without a TTY it prints the authentication URL (skipping the QR code and the "Press ENTER to open the URL in your browser" prompt) and polls the registry until the browser approval completes. Only the classic username/password login still fails with `ERR_PNPM_LOGIN_NON_INTERACTIVE` in a non-interactive terminal.

- Added `projects[].dependencyManifest` to the `@pnpm/napi` install options: the manifest a workspace project exposes when it is resolved as a dependency of another importer (an injected instance). Hosts that pre-transform their importer manifests no longer need a `readPackage` hook to substitute the raw manifest, and per-manifest deletions are expressed through the existing `overrides` removal syntax (`"pkg": "-"`), so resolution can run without any JS round trips.

- The `save-prefix` setting now accepts `=`: newly added dependencies are saved with an explicit `=` operator (`=1.2.3`) instead of the setting being silently treated as the default `^`.

### Patch Changes

- `allowBuilds` entries can now approve git-hosted packages that pnpm downloads as a tarball, such as `github:` dependencies (which are fetched from `codeload.github.com` rather than cloned), by their repository URL without the resolved commit hash. This matches the hashless `git+` matching already supported for cloned git dependencies. For example:

  ```yaml
  allowBuilds:
    "foo@git+https://github.com/org/foo.git": true
  ```

  This approves the package whether pnpm clones it or downloads a tarball, so the entry no longer has to be updated every time the pinned commit changes. GitLab and Bitbucket tarball downloads are matched the same way. Approving or denying a specific resolved commit by its full tarball dep path continues to work.

- Fixed a severe slowdown resolving large workspaces against registries whose abbreviated metadata lacks per-version `time` fields (such as `node-registry.bit.cloud`) while `minimumReleaseAge` is active. The resolver upgraded the abbreviated packument to full metadata once per *dependency edge* instead of once per package — re-requesting the same packument from the registry hundreds of times in a single install — and a `304 Not Modified` answer was never remembered, so the round trip repeated forever. The upgrade outcome is now cached for the rest of the install. On a 345-project workspace this cut a full resolution from 105 s to 36 s.

  Also stopped the resolver from deep-copying every workspace project manifest on each internal resolve-options clone (the workspace-packages map is now shared by reference).

- Aligned deprecated package warnings with pnpm by reporting each package only on its first resolution and shortening direct dependency warnings during recursive installs.

- `pnpm outdated --include-github-actions` no longer blocks on an interactive git credential prompt when a workflow uses a private action repo.

- `pnpm add` and `pnpm update` now honor the `saveExact` setting; previously only the `--save-exact` flag was respected.

- Fixed parsing very large lockfiles that exceed the YAML parser's default 64 MiB scalar-text budget.

- Fixed parsing large lockfiles that exceed the YAML parser's default structural budget [pnpm/pnpm#12857](https://github.com/pnpm/pnpm/issues/12857).

- Fixed writing lockfiles with dependency paths longer than 1024 characters (long peer suffixes in large workspaces): such keys are now emitted in explicit `? <key>` form, matching the TypeScript CLI. Inline keys of that length are invalid YAML, so pnpm could not re-read the lockfile it had just written and every subsequent install re-resolved from scratch.

- Prevented `minimumReleaseAge` from replacing `latest` with a SemVer-greater version than the registry tag target [#13034](https://github.com/pnpm/pnpm/issues/13034).

- Installs driven through `@pnpm/napi` got three fixes for large workspaces: the `readPackage` hook is now dispatched to JavaScript in batches instead of one event-loop roundtrip per manifest, the `dedupePeers` setting can be passed through the install options (so an existing lockfile generated with it is no longer treated as outdated), and version-pinned dependencies are served from the metadata mirror without queueing behind concurrent registry refreshes of the same package.

- Fixed the `overrides` block of `pnpm-lock.yaml` being rewritten in a random order on every install performed through `@pnpm/napi`. The recorded overrides now keep the order they were declared in, so repeat installs no longer churn the lockfile.

- The `TRACE` environment variable now enables engine tracing for `@pnpm/napi` consumers the same way it does for the pnpm CLI, and an invalid `TRACE` filter no longer aborts the process — it prints a warning and leaves tracing off.

- Fixed peer resolution creating far more peer variants than the TypeScript CLI in multi-importer workspaces: a dependency subtree first resolved under one importer no longer hands the peer providers it resolved to every other importer that shares it. Those importers now bind such peers against their own context (or the workspace root), matching the TypeScript resolver. In a large bit.cloud workspace this cut a from-scratch install from 25,534 to 20,791 lockfile snapshots.

- Fixed empty `bundledDependencies` and `bundleDependencies` arrays causing nondeterministic lockfile changes. See pnpm/pnpm#13123.

- Reduced the peer resolution pass's CPU cost on workspaces with many peer dependencies. The walker cloned its parent peer-context maps at every node — twice per node plus once per child — even when a node contributed nothing to them; the maps are now shared copy-on-write and the derived per-child snapshots are reused unless the context actually changed. On a peer-heavy 331-importer benchmark the full resolution dropped from 3.9 s to 2.8 s.

- The install summary no longer prints `(X is available)` when the registry's `dist-tags.latest` is still held back by the active `minimumReleaseAge` policy. The hint only ever names the actual latest tag, so an immature latest suppresses the hint instead of advertising the version pnpm just refused to install [#11698](https://github.com/pnpm/pnpm/issues/11698).

- `pnpm update` keeps the explicit `=` operator of an exact version pin: a dependency saved as `=3.5.1` now updates to `=3.5.2` instead of the bare `3.5.2`. See pnpm/pnpm#13168.

- Preserve a workspace dependency's `link:` entry when a run does not target it — e.g. `pnpm update <other-pkg>` (with or without `--recursive`), or a plain install after a root/catalog dependency change — with `injectWorkspacePackages`, instead of spuriously rewriting it to a peer-suffixed `file:` protocol. See pnpm/pnpm#10433.

- Fixed resolution of a direct dependency declared in both `dependencies` and `devDependencies`: the `dependencies` specifier now wins, matching the TypeScript CLI. The `devDependencies` range was resolved instead, recording a lockfile importer entry whose version did not satisfy its specifier — which failed the lockfile up-to-date check and forced a full re-resolve on every install.

- Kept the lockfile policy verdict ahead of the frozen-install message when package statistics arrive while verification is still running.

- `overrides` are now applied after the `readPackage` hook during resolution, matching the TypeScript CLI's hook order (`packageExtensions` → `readPackage` hooks → `overrides`). A hook that replaced a manifest — such as a host application substituting a workspace project's raw manifest for its injected instances — previously erased the overrides from that manifest, so the resolved graph ignored them.

- Sped up multi-importer resolution by sharing the run-resolved preferred-versions fold across importers. Every importer replayed the whole workspace's resolved-versions history into a private map each hoist round — O(importers × packages) map inserts and string clones — although the peer-hoist pickers only ever look up a handful of missing-peer names. The fold is now maintained once, workspace-wide, and importers materialize just the buckets they query. Full resolution of a 331-importer benchmark workspace dropped from 886 ms to 424 ms (peer-heavy variant: 2.8 s to 2.4 s).

- Fixed quadratic time and memory use when resolving a large multi-project workspace from scratch. Resolving a workspace with hundreds of projects sharing thousands of packages previously took minutes and several gigabytes of memory; it now completes in seconds.
