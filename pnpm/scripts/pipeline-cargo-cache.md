# Cargo build state in a pipeline

The experimental pipeline command can share local Cargo build state between
Git worktrees of the same repository. Enable it for a task in
`pnpm-workspace.yaml`:

```yaml
includeWorkspaceRoot: true
pipelines:
  default: [build]
tasks:
  build:
    dependsOn: []
    cargoTargetDir: target
    env: [MY_BUILD_SETTING]
```

The corresponding project's `package.json` can contain:

```json
{
  "scripts": {
    "build": "cargo build --locked --offline"
  }
}
```

Add `target/` to `.gitignore`, commit the Cargo lockfile, and run
`pnpm pipeline --full`. The normal pipeline frozen-install requirement still
applies. `includeWorkspaceRoot` is needed when the script is at the workspace
root; workspace packages do not need it.

`cargoTargetDir` is relative to the project that runs the script. pnpm sets both
`CARGO_TARGET_DIR` and `CARGO_BUILD_BUILD_DIR` to that directory. Do not override
these variables or pass a different target directory in the script. Give each
Cargo workspace its own target directory. Do not run external Cargo commands
against that directory while the pipeline is executing or publishing a snapshot.
Pipeline invocations using the same target coordinate through a process lock.

Every Cargo task executes, even when a snapshot was restored. Declaring
`cargoTargetDir` disables completed-output cache hits for that task, including
when `outputs` is also declared. `--no-cache` and `cache: false` disable snapshot
reads and writes while retaining the selected local target directory.

A snapshot is restored only into an absent target directory and only for
matching inputs. Existing target directories retain their local incremental
state. Changes to input contents invalidate Cargo freshness records even when
file modification times have not changed. When a snapshot moves between
worktrees, local-package and build-script freshness records are invalidated to
avoid retaining the publishing worktree's paths.

Snapshots live under `<cacheDir>/cargo-build/v1`, separately from installed
packages and the global virtual store. They are scoped to the Git common
directory, so linked worktrees share them but independent clones do not.
Restoration uses reflinks where available and copies otherwise. Neither creates
a live dependency on cached files. Deleting this cache leaves installed packages,
restored targets, and their binaries usable. A missing or corrupt snapshot is a
miss; partial restores stay in a temporary directory and are discarded.

This is an opt-in, local prototype with a bounded input contract:

- Source inputs must be tracked or untracked, unignored files inside the Git
  repository. The snapshot hashes the whole repository input set, including
  sibling path dependencies, regardless of a task's narrower `inputs` globs.
  Symlink/submodule inputs and local Cargo packages outside the repository
  decline caching. Gitignored generated inputs are not covered.
- Rust/Cargo versions, Cargo configuration in parent directories and Cargo home,
  declared task environment, and common Rust/native build environment variables
  participate in the key. Declare additional build-script variables in `env`.
  Native SDKs, external files, databases, and tools modified in place are not
  fully fingerprinted. Disable caching for tasks whose inputs are outside this
  contract.
- Cargo metadata must resolve with `--locked --offline`. If dependencies are
  unavailable or metadata fails, the task still runs without snapshot reuse.
- Snapshots containing symlinks or special files are not published. Remote
  publication, automatic eviction, reuse from a different source revision, and
  physical content deduplication across snapshots are not implemented.

The earlier reflink benchmark did not include these validation and publication
costs or relocation invalidation. Its startup timings are not performance claims
for this implementation.
