# Agent Guide to pnpm Repository

This document provides context and instructions for AI agents working on the pnpm codebase.

## Repository Structure

The pnpm codebase is a monorepo managed by pnpm itself. The root contains functional directories organized by domain:

### Core Directories

-   `pnpm/`: The CLI entry point and main package.
-   `pkg-manager/`: Core package management logic (installation, linking, etc.).
-   `resolving/`: Dependency resolution logic (resolvers for npm, tarballs, git, etc.).
-   `fetching/`: Package fetching logic.
-   `store/`: Store management logic (content-addressable storage).
-   `lockfile/`: Lockfile handling, parsing, and utilities.

### CLI & Configuration

-   `cli/`: CLI command implementations and infrastructure.
-   `config/`: Configuration management and parsing.
-   `hooks/`: pnpm hooks (readPackage, etc.).
-   `completion/`: Shell completion support.

### Other Functional Directories

-   `network/`: Network-related utilities (proxy, fetch, auth).
-   `workspace/`: Workspace-related utilities.
-   `exec/`: Execution-related commands (run, exec, dlx).
-   `env/`: Node.js environment management.
-   `cache/`: Cache-related commands and utilities.
-   `patching/`: Package patching functionality.
-   `reviewing/`: License and dependency review tools.
-   `releasing/`: Release and publishing utilities.

### Shared Utilities

-   `packages/`: Shared utility packages (constants, error handling, logger, types, etc.).
-   `fs/`: Filesystem utilities.
-   `crypto/`: Cryptographic utilities.
-   `text/`: Text processing utilities.

## Setup & Build

To set up the environment and build the project:

```bash
pnpm install
pnpm run compile
```

## Testing

Never run all tests in the repository as it takes a lot of time.

Run tests for a specific project instead:

```bash
# From the project directory
pnpm test

# From the root, filtering by package name
pnpm --filter <package_name> test
```

Or better yet, run tests for a specific file:

```bash
pnpm --filter <package_name> test <file_path>
```

Or a specific test case in a specific file:

```bash
pnpm --filter <package_name> test <file_path> -t <test_name_pattern>
```

## Linting

To run all linting checks:

```bash
pnpm run lint
```

## Contribution Workflow

### Changesets

If your changes affect published packages, you MUST create a changeset file in the `.changeset` directory. The changeset file should describe the change and specify the packages that are affected with the pending version bump types: patch, minor, or major.

**IMPORTANT: Always explicitly include `"pnpm"` in the changeset** with the appropriate version bump (patch, minor, or major). The pnpm CLI will only receive automatic patch bumps from its dependencies, so if your change warrants a minor or major version bump for the CLI, you must specify it explicitly. The changeset description will appear on the release notes page.

Example:

```
---
"@pnpm/core": minor
"pnpm": minor
---

Added a new setting `blockExoticSubdeps` that prevents the resolution of exotic protocols in transitive dependencies [#10352](https://github.com/pnpm/pnpm/issues/10352).
```

**Versioning Guidelines for pnpm CLI:**
- **patch**: Bug fixes, internal refactors, and changes that don't require documentation updates
- **minor**: New features, settings, or commands that should be documented (anything users should know about)
- **major**: Breaking changes

### Commit Messages

Follow the [Conventional Commits](https://www.conventionalcommits.org/) specification.
    -   `feat`: a new feature
    -   `fix`: a bug fix
    -   `docs`: documentation only changes
    -   `style`: formatting, missing semi-colons, etc.
    -   `refactor`: code change that neither fixes a bug nor adds a feature
    -   `perf`: a code change that improves performance
    -   `test`: adding missing tests
    -   `chore`: changes to build process or auxiliary tools

## Code Reuse and Avoiding Duplication

**Before writing new code, always analyze the existing codebase for similar functionality.** This is a large monorepo with many shared utilities — duplication is a real risk.

-   **Search before you write.** Before implementing any non-trivial logic, search the codebase for existing functions, utilities, or patterns that do the same or similar thing. Check `packages/`, `fs/`, `crypto/`, `text/`, and other shared directories first.
-   **Extract shared code.** If you find that the logic you need already exists in another package but is not exported or reusable, refactor it into a shared package rather than duplicating it. If you are adding new code that is similar to code that already exists elsewhere in the repo, move the common parts into a shared package that both locations can use.
-   **Prefer open source packages over custom implementations.** Do not reimplement functionality that is already available as a well-maintained open source package. Use established libraries for common tasks (e.g., path manipulation, string utilities, data structures, schema validation). Only write custom code when no suitable package exists or when the existing packages are too heavy or unmaintained.
-   **Keep the dependency on the right level.** When adding a new open source dependency, add it to the most specific package that needs it, not to the root or to a shared package unless multiple packages depend on it.

## Code Style

This repository uses [Standard Style](https://github.com/standard/standard) with a few modifications:
-   **Trailing commas** are used.
-   **Functions are preferred** over classes.
-   **Functions are declared after they are used** (hoisting is relied upon).
-   **Functions should have no more than two or three arguments.** If a function needs more parameters, use a single options object instead.
-   **Import Order**:
    1.  Standard libraries (e.g., `fs`, `path`).
    2.  External dependencies (sorted alphabetically).
    3.  Relative imports.

To ensure your code adheres to the style guide, run:

```bash
pnpm run lint
```

## Common Gotchas

### Error Type Checking in Jest

When checking if a caught error is an `Error` object, **do not use `instanceof Error`**. Jest runs tests in a VM context where `instanceof` checks can fail across realms.

Instead, use `util.types.isNativeError()`:

```typescript
import util from 'util'

try {
  // ... some operation
} catch (err: unknown) {
  // ❌ Wrong - may fail in Jest
  if (err instanceof Error && 'code' in err && err.code === 'ENOENT') {
    return null
  }
  
  // ✅ Correct - works across realms
  if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
    return null
  }
  throw err
}
```

## Key Configuration Files

-   `pnpm-workspace.yaml`: Defines the workspace structure.
-   `package.json` (root): Root scripts and devDependencies.
-   `CONTRIBUTING.md`: Detailed contribution guidelines.
