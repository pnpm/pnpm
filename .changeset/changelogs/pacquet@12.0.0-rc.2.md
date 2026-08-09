## 12.0.0-rc.2

### Minor Changes

- Globally installed bins can now follow the project you run them in. The new `globalShims` setting is a record of package names to policies that selects which globally installed packages get project-aware shims; it defaults to `{ node: true, deno: true, bun: true }` and merges key-wise, so `globalShims: { bun: false }` switches one default off and `globalShims: { typescript: true }` adds another package. With the default, a project that pins Node.js through `devEngines.runtime` or `engines.runtime` gets the pinned stable release — authenticated against the Node.js release-team signatures — downloaded on first use and run whenever you type `node` inside the project, with no shell hooks. Candidates that are not signature-verified (Deno, Bun, Node.js prereleases, and ordinary package bins you enable) ask "Do you trust this project?" once per candidate and remember the answer machine-locally; the record values name the policy per package: `"auto"` (or its shorthand `true`) defers to artifact authentication, `"always"` switches without ever asking (useful in CI), and `"prompt"` always asks, even for authenticated candidates. Set `globalShims: false` to disable the feature, or `PNPM_SHIM_BYPASS=1` to bypass it for one invocation. On Windows, programs can keep spawning the global `node.exe` directly, without a shell.

### Patch Changes

- `pnpm dlx` and `pnpm create` no longer fail with "Failed to read patch file" in a project that has `patchedDependencies`. As in pnpm, the package dlx runs is installed unpatched.

- Reduced the warm startup overhead of project-aware managed runtime shims.

- `ng build` and `nuxt build` now work under the global virtual store: pnpm's built-in compatibility extensions add the `tslib` dependency that `@angular/build` uses without declaring and the `unplugin` dependency that `@nuxt/vite-builder` v4 uses without declaring.

- The automatic `packageManager` version switch works again on registries whose tarball URLs point at a different host than the registry itself (load-balanced feed proxies, Artifactory-style mirrors). Package-manager entries are now always recorded with integrity-only resolutions — the download URL is derived from the trusted bootstrap registry instead — and entries persisted in an invalid shape by an earlier pnpm are discarded and re-resolved instead of failing every command [#13619](https://github.com/pnpm/pnpm/issues/13619).

- Registries that serve no npm signature metadata (private mirrors and feed proxies commonly strip `dist.signatures`) no longer break the automatic `packageManager` version switch and `pnpm self-update` [#13147](https://github.com/pnpm/pnpm/issues/13147). When the configured registry cannot provide a verifiable signature, pnpm now fetches the signature from `registry.npmjs.org` and verifies it against the same embedded npm keys over the installed integrity — which proves exactly the same thing. If no signature can be obtained from either source (for example, both are unreachable, or the registry publishes only a `shasum`), pnpm proceeds with a warning instead of failing, but only when the packages resolve through a registry configured in the user's own (non-project) configuration; the download stays pinned by the lockfile integrity, and a signature that exists but does not validate still fails the switch.

- `pnpm setup` no longer makes Node.js print a `MODULE_TYPELESS_PACKAGE_JSON` warning about `dist/worker.js` on every command. The `package.json` it writes next to a standalone executable now declares `"type": "module"`.
