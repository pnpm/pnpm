## 12.9.1

This release moves the WebContainer build into a separate `@pnpm/wasm` package, shrinks the `pnpm` package back to about 4 MB, and fixes `pnpm publish` with provenance from GitLab CI.

### Patch Changes

- The WebAssembly build for StackBlitz WebContainers now ships as a separate `@pnpm/wasm` package. The `pnpm` and `@pnpm/exe` packages no longer include it, which brings their unpacked size back from about 55 MB to about 4 MB. In a WebContainer, install `@pnpm/wasm` with npm to get the `pnpm` command.

- `pnpm publish` with provenance from GitLab CI is no longer rejected by the npm registry with a 422 error. The provenance statement now includes the GitLab CI variables in `invocation.parameters`, as npm does [#16551](https://github.com/pnpm/pnpm/issues/16551).

- `pnpm audit signatures` now uses the TLS settings of the redirect target when a registry redirects its signing-keys request, for example to registry.npmjs.org. A `cafile` scoped to a private registry no longer makes the redirected request fail [#16541](https://github.com/pnpm/pnpm/issues/16541).

- Fixed `pnpm install --frozen-lockfile` rejecting an up-to-date lockfile when an injected workspace package uses a catalog entry in `peerDependencies` [#16557](https://github.com/pnpm/pnpm/issues/16557).

- The `[<since>]` filter selector works again with Git 2.24 through 2.27 [#16561](https://github.com/pnpm/pnpm/issues/16561). With Git older than 2.24, the selector now fails with an error that names the required Git version.

  It also detects changes in projects whose directory names contain non-ASCII characters. Such a change used to be credited to the parent project. `changedFilesIgnorePattern` and `testPattern` now match changed files whose names contain non-ASCII characters.

- The `pnpm` executable is about 10% smaller. On macOS arm64 it went from 45.1 MB to 40.3 MB.

- Sped up trust downgrade checks for packages with long release histories.

- With `optimisticRepeatInstall: false`, `pnpm install` now runs the projects' own lifecycle scripts, such as `prepare`, even when `node_modules` is already up to date [#16545](https://github.com/pnpm/pnpm/issues/16545).

- `pnpm self-update` now fails for Homebrew-installed pnpm and prints the `brew upgrade` command for the installed formula, such as `brew upgrade pnpm` or `brew upgrade pnpm@11`. It used to install a second copy of pnpm that the Homebrew one kept shadowing [#16547](https://github.com/pnpm/pnpm/issues/16547).
