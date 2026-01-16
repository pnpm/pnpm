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

1.  **Changesets**: If your changes affect published packages, you MUST create a changeset file in the `.changeset` directory. The changeset file should describe the change and specify the packages that are affected with the pending version bump types: patch, minor, or major. For example:

```
---
"@pnpm/core": minor
"pnpm": patch
---

Added a new setting `blockExoticSubdeps` that prevents the resolution of exotic protocols in transitive dependencies [#10352](https://github.com/pnpm/pnpm/issues/10352).
```

Always specify the "pnpm" package, even if it wasn't directly changed. This text will appear on the release page.

2.  **Commit Messages**: Follow the [Conventional Commits](https://www.conventionalcommits.org/) specification.
    -   `feat`: a new feature
    -   `fix`: a bug fix
    -   `docs`: documentation only changes
    -   `style`: formatting, missing semi-colons, etc.
    -   `refactor`: code change that neither fixes a bug nor adds a feature
    -   `perf`: a code change that improves performance
    -   `test`: adding missing tests
    -   `chore`: changes to build process or auxiliary tools

## Code Style

This repository uses [Standard Style](https://github.com/standard/standard) with a few modifications:
-   **Trailing commas** are used.
-   **Functions are preferred** over classes.
-   **Functions are declared after they are used** (hoisting is relied upon).
-   **Import Order**:
    1.  Standard libraries (e.g., `fs`, `path`).
    2.  External dependencies (sorted alphabetically).
    3.  Relative imports.

To ensure your code adheres to the style guide, run:

```bash
pnpm run lint
```

## Key Configuration Files

-   `pnpm-workspace.yaml`: Defines the workspace structure.
-   `package.json` (root): Root scripts and devDependencies.
-   `CONTRIBUTING.md`: Detailed contribution guidelines.
