---
title: Store loader compatibility results
---

These are scoped experiments with the [experimental store loader](./store-loader.md), not full repository test passes. The external projects come from [Workspaces in the wild](https://pnpm.io/workspaces#workspaces-in-the-wild). Results were collected on Linux x64 with Node.js 26.10.0. The latest batch, recorded on October 1, 2026, adds Prisma, Verdaccio, Logto, and Stimulus Components.

## Method

Each workload first runs against a normal installation. The diagnostic then copies workspace sources and built outputs, snapshots installed registry packages into CAS, and reruns the same workload with the loader. It checks that the copied tree contains no `node_modules` directory or symlink outside the tree. The original checkout retains its normal installation for the baseline.

In the all-CAS run, registry packages have only store blobs and manifest entries. Workspace sources remain physical. In the GVS run, explicitly selected packages and their full locked dependency trees have normal GVS installations outside the copied application. GVS and the retained staging installation contain `node_modules`; the copied application does not. Installation scripts are disabled for these experiments.

A snapshot's package count describes available dependencies, not test coverage. The audit counts distinct module URLs successfully loaded through Node's hooks and their CAS package instances, including inherited worker preloads. It does not count sources that a runner reads directly into its own VM. A failed startup can load CAS modules without executing any tests.

## Results

| Project and workload | Normal baseline | All-CAS result | GVS opt-out result |
| --- | --- | --- | --- |
| pnpm CLI argument parser | 51 passed | Jest reads a virtual source path through `fs`; 0 tests | 51 passed with the explicit selections below; 362 package instances mapped to 360 GVS directories |
| Vue reactivity | 445 passed, 4 skipped | Native Rolldown binding required; 0 tests | Same baseline result with `vitest` and its tree: 136 instances |
| Vite utilities | 131 passed | Native Rolldown binding required; 0 tests | Diagnostic rejects Vitest's dependency on the local Vite workspace |
| Svelte stores | 33 passed | Vite reads its virtual `package.json` through `fs`; 0 tests | `vitest` and its tree: 128 instances; Vite cannot resolve `esm-env`, 0 tests |
| Svelte compiler corpus | 200 compilations | All generated outputs match; 297 CAS modules from 15 packages | Not needed |
| Astro HTML plugin, routing generator, and utilities | 116 passed in 12 suites | 116 passed; 4 CAS modules from 4 packages | Not needed |
| Nuxt pages, utilities, plugin utilities, and kit utilities | 87 passed in 4 files | Native Rolldown binding required; 0 tests | `vitest` and its tree: 118 instances; configuration cannot resolve `@nuxt/kit`, 0 tests |
| VueUse shared utilities | 318 passed, 1 todo in 58 files | Native Rolldown binding required; 0 tests | `vitest` and its tree: 178 instances; all 58 suites fail to resolve `msw` in setup, 0 tests |
| Mermaid parser | 294 passed in 11 files | Vite reads its virtual `package.json` through `fs`; 0 tests | `vitest` and its tree: 153 instances; configuration cannot resolve `jison`, 0 tests |
| SvelteKit utilities | 274 passed in 12 files | Native Rolldown binding required; 0 tests | `vitest` and its tree: 98 instances; 49 tests pass, 6 suites fail on dependency resolution |
| Slidev parser | 114 passed in 4 files | Native Rolldown binding required; 0 tests | `vitest` and its tree: 52 instances; 3 tests pass, 2 suites cannot resolve `@antfu/utils` |
| NextAuth.js JWT, URL parsing, merge, and environment utilities | 57 passed in 4 files | Native Rollup binding required; 0 tests | `vitest`, `vite`, and `unplugin-swc` trees: 108 instances; 51 tests pass, JWT suite cannot resolve `@panva/hkdf` |
| Rollup plugin-utils | 90 passed in 8 files | Vite reads its virtual `package.json` through `fs`; 0 tests | `vitest` and its tree: 57 instances; 8 suites fail on `estree-walker` or `acorn`, 0 tests |
| Kysely database-free Mocha units | 25 passed in 8 files | 25 passed; 656 CAS modules from 87 packages | Not needed |
| Kysely file migrations | 10 passed | 5 pass before the `.cts` case stalls; terminated after three minutes | `esbuild` trees: 4 instances; all 10 pass, 666 CAS modules from 87 packages |
| Milkdown context | 15 passed in 5 files | Native Rolldown binding required; 0 tests | `vitest` and its tree: 89 instances; 3 tests pass, 4 suites cannot resolve the exception workspace |
| Milkdown transformer | 21 passed in 5 files | Native Rolldown binding required; 0 tests | Same 89-instance Vitest tree; 5 suites cannot resolve the exception workspace, 0 tests |
| Element Plus utilities | 107 passed in 20 files | Native Rolldown binding required; 0 tests | `vitest` and its tree: 195 instances; all 20 suites fail to resolve test setup dependencies, 0 tests |
| Quasar CLI units | 57 passed in 7 files | Native Rolldown binding required; 0 tests | `vitest` and its tree: 128 instances; 25 tests pass, 4 fail, 3 other suites fail during import |
| Quasar SSR error utilities | 3 passed in 2 files | Native Rolldown binding required; 0 tests | Same 128-instance Vitest tree; 1 test passes, 1 suite cannot resolve `stack-trace` |
| Qwik shared utilities, tag nesting, and preload utilities | 167 passed in 8 files | Native Rolldown binding required; 0 tests | `vitest`, `oxc-parser`, and `oxc-transform` trees: 93 instances; all 167 pass, 19 CAS modules from 8 packages |
| Turborepo utilities | 33 passed and 5 snapshots in 6 files | Jest cannot locate the local test-utils preset; 0 tests | Diagnostic rejects conflicting Babel GVS contexts before running tests |
| Cycle.js run | 32 passed | Extensionless Mocha executable is unsupported; 0 tests | Diagnostic rejects lockfile format 5.3 before installation |
| ByteMD editor and viewer | 7 passed in 2 files with browser resolution | Vite reads its virtual `package.json` through `fs`; 0 tests | Diagnostic rejects lockfile format 6.0 before installation |
| Prisma foundation utilities | 169 runtime tests passed in 14 files | Native Rolldown binding required; 0 tests | Vitest trees: 119 instances; all 14 suites fail to locate a workspace tsconfig |
| Prisma Node-based coverage tools | 89 passed in 10 suites | 89 passed; 160 CAS modules from 12 packages | Not needed |
| Verdaccio core and URL helpers | 166 passed, 1 skipped in 17 files | Native Rolldown binding required; 0 tests | `vitest` and its tree: 157 instances; all 17 suites fail to resolve `nock` in setup |
| Logto shared utilities | 71 passed in 9 files | Native Rollup binding required; 0 tests | `vitest` and shared tsconfig trees: 149 instances; 7 tests pass, 7 suites fail on dependency resolution |
| Stimulus Components DOM tests | 154 passed in 22 files | Vite reads its virtual `package.json` through `fs`; 0 tests | `vitest` and its tree: 96 instances; all 22 suites fail to resolve `@hotwired/stimulus` |

### What the passing runs establish

Astro's selected tests exercise compiled workspace code with Node's test runner and no materialized registry packages. Only four registry modules were observed through the CAS hooks. The snapshot contains 1,951 registry package instances and 565 workspace roots, so this is deliberately narrow coverage of that graph.

The Svelte comparison compiles 100 real repository components for both client and server. Hashes of generated JavaScript, CSS, source maps, and warnings match the baseline. This bypasses Vitest and does not establish that Svelte's test runner works with all-CAS dependencies.

Kysely's Mocha run exercises query IDs, logging, object utilities, JSON-result parsing, immediate values, plugin composition, and async disposal. The tests use dummy drivers, not live databases. Its 656 observed CAS modules include Mocha, assertion and mocking libraries, and database-client imports from shared test setup. No registry packages are materialized. The snapshot contains 1,592 registry instances and four workspace roots; this is not coverage of every dependency or of database integration.

Kysely's separate file-migration suite passes all ten tests with only the two esbuild versions and their platform binaries in GVS (four instances). Mocha and the remaining dependencies stay in CAS. The all-CAS run stalls while loading a `.cts` migration through the TypeScript tooling after five passing cases. Selecting esbuild resolves that stall; the passing run observes 666 CAS modules from 87 packages.

Prisma's separate Node-runner workload exercises its coverage report, upgrade-coverage gate, and workflow checks. All 89 tests pass with no registry materialization, loading 160 CAS modules from 12 package instances, including schema validation and Markdown frontmatter dependencies. Tests that create temporary Git repositories and launch child processes also pass. This does not cover Prisma database access or its Vitest suites.

Qwik's eight selected files pass all 167 tests after opting out Vitest and both installed versions of `oxc-parser` and `oxc-transform`. Vitest alone materializes 76 instances but fails on the parser's native binding; adding the parser reaches the transformer's native binding. The final 93-instance selection loads 19 modules from eight CAS package instances. This exercises built workspace code and source tests, not the full framework or browser suite.

Vue's reactivity suite and pnpm's parser suite pass with GVS fallback. Vue's audited GVS rerun observed no CAS module loads: its passing workload uses physical workspaces and materialized tooling. The pnpm parser needs test setup and application dependencies in addition to Jest's own tree. These passes do not establish that opting out only the test runner suffices in other projects.

### Remaining failures

- **Nuxt:** configuration imports `@nuxt/test-utils`, whose `exsolve` lookup cannot locate the local `@nuxt/kit` workspace without filesystem dependency links. Node resolution with the loader finds `packages/kit/src/index.ts`. The failed configuration run still loads 36 CAS modules from 20 packages.
- **VueUse:** Vite's resolver cannot find `msw` or `msw/node` from `packages/.test/mockServer.ts`. The selected Vitest tree already materializes `msw` in GVS. Node resolution with the loader finds it there, but Vite's own resolver does not consult that mapping. The failed run loads 28 CAS modules from 12 packages.
- **Mermaid:** Vite's config runner cannot resolve `jison` imported by `.build/jisonTransformer.ts`. Node resolution with the loader finds its CAS module. No CAS modules are observed before the config-runner failure in the GVS attempt.
- **Svelte and Vite:** Node resolution with the loader finds `esm-env` and `magic-string`. Their test runners' independent resolution paths still fail. Vite's `magic-string` failure was observed in the earlier individual-package experiment; the GVS helper stops sooner because its opt-out graph contains a workspace link.
- **SvelteKit:** six utility suites fail on Vite resolution of `magic-string`, `esm-env`, or `valibot`; six other suites pass. Node resolution with the loader finds all three dependencies, including `magic-string` already in GVS. The failed run loads 554 CAS modules from 15 packages, primarily through configuration tooling. A diagnostic config moves the Svelte plugin's Vite cache outside `node_modules`; without it, the original config also fails the isolation check despite `--no-cache`.
- **Slidev:** the parser fixture suite and parser utility suite cannot resolve `@antfu/utils`. Two time-parser suites pass three tests. Node resolution with the loader finds the missing dependency. No CAS module loads are observed in the GVS attempt.
- **NextAuth.js:** opting out only Vitest installs 91 instances but leaves a second Vite context in CAS; configuration reads that context's virtual `package.json`. Selecting `vitest`, `vite`, and `unplugin-swc` gets through configuration and passes three suites. The JWT suite still fails in Vite's resolver on `@panva/hkdf`. Node with the loader imports that package from CAS and successfully derives a key. The partial test run loads 406 CAS modules from 38 packages. A separate attempt including `@preact/preset-vite` was rejected by the diagnostic because one original Babel package instance mapped to conflicting GVS roots; that attempt executed no tests.
- **Rollup plugins:** the compiled plugin-utils package imports `estree-walker`, and its scope tests import `acorn`. Vite cannot resolve either, though Node with the loader finds both and `acorn` is already in GVS. No CAS modules are observed in the GVS attempt.
- **Milkdown:** both probes fail in Vite's resolver on `@milkdown/exception`, a physical workspace package. ESM resolution with the loader finds its TypeScript entry, but importing that entry directly through Node then fails on its extensionless relative imports; it still needs the project's TypeScript tooling. The three passing context tests do not load CAS modules. No CAS module loads are observed in either GVS probe.
- **Element Plus:** all utility suites fail in setup because Vite cannot resolve `@vue/test-utils`. Node resolution with the loader finds it in CAS. Configuration startup loads 310 CAS modules from 44 packages before the suites fail.
- **Quasar:** Vite cannot resolve `ci-info`, `cross-spawn`, or `stack-trace` in the selected suites, though Node with the loader resolves them. The CLI also has four assertion failures: a resolved path no longer contains the package name `kolorist`, reading that package's virtual `package.json` through `fs` returns no data, and two subprocess tests cannot discover newly created local CLI packages through `require.resolve(..., { paths })`. Those temporary installations are outside the manifest. The CLI probe observes 28 CAS modules from 16 packages; the SSR utility probe observes none.
- **Turborepo:** Jest's preset lookup cannot locate the physical `@turbo/test-utils` workspace; the all-CAS attempt loads 61 modules from 47 packages before configuration fails. The first attempt also exposed a loader defect: CommonJS `require('..')` was rejected even when the target stayed inside the stored package. The loader now accepts `require('.')` and `require('..')` within that boundary, with regression coverage that still rejects escape from the package root. The preset failure is the result after that fix. Opting out Jest is rejected because one original `@babel/core` instance maps to conflicting GVS roots; no tests run in that attempt.
- **Cycle.js:** the normal Mocha 6 suite passes after installing the existing lockfile with pnpm 6.35.1 under Node.js 18.20.8. Baseline tests and loader probes both use Node.js 26.10.0. The loader rejects Mocha's extensionless `bin/mocha` before any CAS modules load. Opting out Mocha and ts-node cannot be tested with the GVS helper because the installed lockfile uses format 5.3.
- **ByteMD:** seven tests pass after a diagnostic config selects Svelte's browser condition. Without it, three tests fail because mount hooks resolve to server-side behavior. The all-CAS run loads 91 modules from 40 packages before Vite's direct `package.json` read fails. The GVS helper rejects the installed format-6.0 lockfile. This is a diagnostic limitation, not evidence that a correctly materialized Vitest tree would fail.
- **Prisma:** the selected foundation package extends `@repo/tsconfig/base`. Vite's Oxc transform cannot locate that tsconfig without dependency links, so all 14 suites fail before running tests. Node resolution with the loader finds the existing physical `packages/0-config/tsconfig/base.json` workspace file. The Vitest selection includes both installed runner versions (5.0.0-rc.2 and 4.1.10), totaling 119 instances; no CAS modules are observed in this GVS attempt. This checkout is the Prisma 8 development branch, not a released Prisma 7 workload. Type-only tests are excluded from this runtime probe.
- **Verdaccio:** every selected suite fails while importing `nock` from the root test setup. Node resolution with the loader finds its CAS entry. The setup disables external network connections, so it is kept intact. No CAS module loads are observed in the GVS attempt.
- **Logto:** Vitest's 145-instance tree initially reaches an `fs` read of the shared tsconfig's virtual JSON file. Selecting `@silverhand/ts-config` as well materializes four more instances and gets past that read. Seven tests in two suites then pass; seven other suites cannot resolve `nanoid`, `libphonenumber-js`, `ua-parser-js`, or `@silverhand/essentials`. Node resolution with the loader finds all four, and the tsconfig resolves to a physical GVS JSON file. No CAS modules are observed in either GVS attempt. The selected baseline also passes on Node.js 26.10.0 despite the package declaring Node.js 22.
- **Stimulus Components:** all 22 suites fail in Vite's import analysis on `@hotwired/stimulus`. Node resolution with the loader finds its CAS entry. No tests or CAS module loads are observed in the GVS attempt.

The GVS fallback addresses physical-file requirements within an installed dependency tree. It does not make a separate resolver understand application dependencies in the loader manifest. The custom-resolver failures need resolver integration or additional physical application links, not just more copies of package files.

The pnpm CLI compatibility probe also reached registry setup, which requires the external `pnpr-prepare` binary. Its full CLI suite has not passed in this setup. Unbundled CLI startup with `with current --version` and `with current help` succeeded.

## Pinned checkouts

| Project | Revision | Pinned pnpm | Test runner |
| --- | --- | --- | --- |
| Vue | [4ab865a848a1da3d10fb674f857e5fff13094644](https://github.com/vuejs/core/tree/4ab865a848a1da3d10fb674f857e5fff13094644) | 12.4.2 | Vitest 4.1.11 |
| Vite | [10033218d239c927cdc375970b5741cce408e81b](https://github.com/vitejs/vite/tree/10033218d239c927cdc375970b5741cce408e81b) | 12.6.0 | Vitest 5.0.2 |
| Svelte | [020242d6bef059df9ae8c13dc8dbff4c9b31e0ff](https://github.com/sveltejs/svelte/tree/020242d6bef059df9ae8c13dc8dbff4c9b31e0ff) | 10.33.4 | Vitest 4.1.7 |
| Astro | [64e40396dc7e955986fd6d7bbd195f72556933d5](https://github.com/withastro/astro/tree/64e40396dc7e955986fd6d7bbd195f72556933d5) | 11.27.0 | Node test runner |
| Nuxt | [3fde4d625cc353e83ac10f6b75db8ca480167be4](https://github.com/nuxt/nuxt/tree/3fde4d625cc353e83ac10f6b75db8ca480167be4) | 12.6.0 | Vitest 5.0.2 |
| VueUse | [efdd69a1481205051e85d9c815eaa5840237f2d1](https://github.com/vueuse/vueuse/tree/efdd69a1481205051e85d9c815eaa5840237f2d1) | 11.25.0 | Vitest 4.1.11 |
| Mermaid | [47fffa05c044f460f8b9645c7a24429ed200a084](https://github.com/mermaid-js/mermaid/tree/47fffa05c044f460f8b9645c7a24429ed200a084) | 10.30.3 | Vitest 3.2.7 |
| SvelteKit | [582f1015e83dc208780c57682a37dc0f7c6c9203](https://github.com/sveltejs/kit/tree/582f1015e83dc208780c57682a37dc0f7c6c9203) | 12.5.0 | Vitest 5.0.1 |
| Slidev | [30a0a54c8739b4b395d9b336a6304cc8ebcc3947](https://github.com/slidevjs/slidev/tree/30a0a54c8739b4b395d9b336a6304cc8ebcc3947) | 12.4.2 | Vitest 5.0.1 |
| NextAuth.js | [a1a16a5a7780488c7449feece410033f445d0b31](https://github.com/nextauthjs/next-auth/tree/a1a16a5a7780488c7449feece410033f445d0b31) | 9.2.0 | Vitest 3.2.6 |
| Rollup plugins | [639f45638234c1c3fabfb13615c78bebaef89ef2](https://github.com/rollup/plugins/tree/639f45638234c1c3fabfb13615c78bebaef89ef2) | 9.5.0 | Vitest 4.0.2 |
| Kysely | [996eae17fe325968da56e5aa4d9d3a216adb120d](https://github.com/kysely-org/kysely/tree/996eae17fe325968da56e5aa4d9d3a216adb120d) | 11.21.0 | Mocha 12.0.1 |
| Milkdown | [bbb8bcbbb8ce0aa16f92567c23f6236747be9b3b](https://github.com/Milkdown/milkdown/tree/bbb8bcbbb8ce0aa16f92567c23f6236747be9b3b) | 12.5.1 | Vitest 5.0.1 |
| Element Plus | [53936e70ef37cd03c2925e812b342c6adcb3f118](https://github.com/element-plus/element-plus/tree/53936e70ef37cd03c2925e812b342c6adcb3f118) | 12.8.1 | Vitest 5.0.2 |
| Quasar | [0fd7ce30e7dd04c6be23e7bd47547800697ee128](https://github.com/quasarframework/quasar/tree/0fd7ce30e7dd04c6be23e7bd47547800697ee128) | 12.3.4 | Vitest 5.0.1 |
| Qwik | [83a98c261d9d0977fe3a862e828d16c965b72b1e](https://github.com/QwikDev/qwik/tree/83a98c261d9d0977fe3a862e828d16c965b72b1e) | 11.22.0 | Vitest 5.0.0 |
| Turborepo | [8cc3cce30705a88ac9020d1adc2bf06f391fdf2d](https://github.com/vercel/turborepo/tree/8cc3cce30705a88ac9020d1adc2bf06f391fdf2d) | 12.0.0 | Jest 30.3.0 |
| Cycle.js | [5ece2a48c3659538208da3dc8d43a142bc0d91a7](https://github.com/cyclejs/cyclejs/tree/5ece2a48c3659538208da3dc8d43a142bc0d91a7) | 6.35.1 (diagnostic selection) | Mocha 6.2.0 |
| ByteMD | [2a3046a510ba6e5b9d8cce63a38e8258ce6e5430](https://github.com/bytedance/bytemd/tree/2a3046a510ba6e5b9d8cce63a38e8258ce6e5430) | 8.15.9 | Vitest 0.29.8 |
| Prisma | [84b3bb693c395ec21ccf2de97d5db4787c805d03](https://github.com/prisma/prisma/tree/84b3bb693c395ec21ccf2de97d5db4787c805d03) | 10.27.0 | Vitest 5.0.0-rc.2 and Node test runner |
| Verdaccio | [83eb46cd8104bec30f8fd379f5138f0da10df5c4](https://github.com/verdaccio/verdaccio/tree/83eb46cd8104bec30f8fd379f5138f0da10df5c4) | 12.3.4 | Vitest 4.1.11 |
| Logto | [c814034401b0e13a6f461e0b0835ad3bbb84c9c2](https://github.com/logto-io/logto/tree/c814034401b0e13a6f461e0b0835ad3bbb84c9c2) | 10.30.3 (diagnostic selection) | Vitest 4.1.11 |
| Stimulus Components | [0ec0b24e911438fee4d036ac2443f934a62d05c7](https://github.com/stimulus-components/stimulus-components/tree/0ec0b24e911438fee4d036ac2443f934a62d05c7) | 10.30.3 | Vitest 4.1.11 |

The pnpm GVS run used this feature branch at `5d922b13566dbbc4142af415149ff73714ec8619`. The external probes use installed dependency graphs, including existing build outputs. They do not test a fresh install directly into CAS.

## Reproduction

Use Node.js 26.10.0 for the diagnostic process; the scripts reuse that executable for both baseline and loader runs. Install each pinned checkout with `pnpm install --frozen-lockfile --ignore-scripts`. From the external checkout, prepare the workloads that require generated files:

```sh
# Vite
pnpm --filter vite run build-bundle

# Astro
pnpm -C packages/astro run build:ci
pnpm --filter @astrojs/internal-helpers run build:ci

# Mermaid
pnpm --filter @mermaid-js/parser langium:generate

# NextAuth.js
pnpm --filter @auth/core build

# Rollup plugins
pnpm --filter @rollup/pluginutils build

# Kysely
pnpm build
pnpm test:node:build

# Qwik
pnpm build.core.dev

# Verdaccio
pnpm --filter @verdaccio/core... --filter @verdaccio/url... build
```

Kysely and Element Plus needed `--config.@pnpm:registry=https://registry.npmjs.org/` on their install command to override the test machine's scoped registry mirror. The mirror omitted timestamps required by Kysely's trust policy and served an Element Plus dependency tarball whose integrity did not match the lockfile. The official registry passed the original frozen-lockfile, integrity, and policy checks. No package versions or trust policies were changed.

For Mermaid, add a diagnostic-only `compatibility.workspace.json` containing `["./vite.config.ts"]`. This selects the existing root configuration without loading the unrelated documentation configuration. The latter uses `__dirname`, which fails under the config runner. Mermaid needs `--configLoader runner` because its configuration imports TypeScript sources through `.js` specifiers. No upstream test sources were changed.

For SvelteKit, add `packages/kit/compatibility.config.js` to redirect the cache while preserving its existing test configuration:

```js
import config from './vitest.kit.config.js';
export default { ...config, cacheDir: '.cache/loader-compatibility' };
```

The snapshot excludes `.cache`, so the loader run does not reuse the baseline's generated dependency cache. The NextAuth.js probe disables coverage for both baseline and loader runs. Its core build supplies the generated `jwt.js` imported by the existing JWT tests. The Rollup plugin-utils tests likewise require built outputs. The diagnostic supports Vitest, Mocha, Jest, and Node's test runner. Package runners are located from the selected working directory using their declared binary entry, including older Mocha releases. The diagnostic's `--cwd` option runs package-specific configurations from the same relative directory in the baseline and copied checkout; it still snapshots the whole workspace.

For ByteMD, keep both the checkout and retained fixture outside any `.cache` directory. Vitest 0.29.8's default exclusion otherwise matches their absolute paths and reports no tests. Set `TMPDIR=/tmp` for its diagnostic and add `compatibility.config.mjs`:

```js
import config from './vitest.config.mjs'
export default { ...config, resolve: { ...config.resolve, conditions: ['browser'] } }
```

This changes browser/server resolution for both baseline and loader runs without modifying test assertions. Cycle.js has no package-manager pin; use pnpm 6.35.1 with Node.js 18.20.8 for its frozen installation, then add `"packageManager": "pnpm@6.35.1"` to the diagnostic checkout's root manifest so workspace discovery uses the matching CLI. Its lockfile and dependency versions remain unchanged. Running pnpm 6's installer on the test machine's newer Node version fails its HTTP requests with `ERR_INVALID_THIS`.

Prisma's Node-runner probe sets `GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=tag.gpgSign GIT_CONFIG_VALUE_0=false` for the test process. The tests create temporary Git tags; inheriting the machine's signed-tag setting otherwise opens an editor. This does not change global Git configuration. The test sources and assertions are unchanged.

Logto does not pin pnpm in its manifest; its CI selects pnpm 10. The diagnostic uses `pnpm with 10.30.3 install --frozen-lockfile --ignore-scripts`. Prisma's foundation probe disables Vitest's type-only tests explicitly; the reported 169 tests check runtime behavior.

Run the following from the pnpm repository, with its workspace utilities installed and compiled:

```sh
node pnpm/esm-loader/scripts/test-ecosystem.mjs /path/to/vue run --project unit packages/reactivity/__tests__ --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs /path/to/vite run packages/vite/src/node/__tests__/utils.spec.ts --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs /path/to/svelte run packages/svelte/tests/store/test.ts --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs --node /path/to/astro --test --test-concurrency=2 'packages/astro/test/units/vite-plugin-html/*.test.ts' packages/astro/test/units/routing/generator.test.ts 'packages/astro/test/units/util/*.test.ts'
node pnpm/esm-loader/scripts/test-ecosystem.mjs /path/to/nuxt run --project unit packages/nuxt/test/pages.test.ts packages/nuxt/test/utils.test.ts packages/nuxt/test/plugin-utils.test.ts packages/kit/test/utils.test.ts --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs /path/to/vueuse run --project unit packages/shared --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs /path/to/mermaid run packages/parser/tests --workspace compatibility.workspace.json --maxWorkers=2 --configLoader runner --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs --cwd packages/kit /path/to/sveltekit run --config compatibility.config.js --project kit-server-dev src/utils --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs /path/to/slidev run packages/parser test/parser.test.ts --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs --cwd packages/core /path/to/nextauth run --config ../utils/vitest.config.ts test/jwt.test.ts test/url-parsing.test.ts test/merge.test.ts test/env.test.ts --coverage.enabled=false --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs --cwd packages/pluginutils /path/to/rollup-plugins run --config ../../.config/vitest.config.mts --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs --mocha /path/to/kysely --timeout 15000 test/node/dist/query-id.test.js test/node/dist/log-once.test.js test/node/dist/object-util.test.js test/node/dist/parse-json-results-plugin.test.js test/node/dist/immediate-value-plugin.test.js test/node/dist/plugin-composition.test.js test/node/dist/async-dispose.test.js test/node/dist/logging.test.js
node pnpm/esm-loader/scripts/test-ecosystem.mjs --mocha /path/to/kysely --timeout 15000 test/node/dist/file-migration-provider.test.js
node pnpm/esm-loader/scripts/test-ecosystem.mjs --cwd packages/ctx /path/to/milkdown run --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs --cwd packages/transformer /path/to/milkdown run --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs /path/to/element-plus run packages/utils/__tests__ --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs --cwd cli /path/to/quasar run --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs --cwd utils/render-ssr-error /path/to/quasar run --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs /path/to/qwik run packages/qwik/src/core/shared/utils packages/qwik/src/server/tag-nesting.unit.ts packages/qwik/src/server/preload-utils.unit.ts --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs --jest --cwd packages/turbo-utils /path/to/turborepo --runInBand --no-cache --runTestsByPath __tests__/convert-case.test.ts __tests__/search-up.test.ts __tests__/is-folder-empty.test.ts __tests__/validate-directory.test.ts __tests__/get-turbo-root.test.ts __tests__/get-turbo-configs.test.ts
node pnpm/esm-loader/scripts/test-ecosystem.mjs --mocha --cwd run /path/to/cyclejs 'test/*.ts' --require ts-node/register --exit
TMPDIR=/tmp node pnpm/esm-loader/scripts/test-ecosystem.mjs /path/outside-cache/bytemd run --threads=false --config compatibility.config.mjs
node pnpm/esm-loader/scripts/test-ecosystem.mjs --cwd packages/1-framework/0-foundation/utils /path/to/prisma run --typecheck.enabled=false --maxWorkers=2 --configLoader native --no-cache
GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=tag.gpgSign GIT_CONFIG_VALUE_0=false node pnpm/esm-loader/scripts/test-ecosystem.mjs --node /path/to/prisma --test --test-concurrency=2 scripts/check-upgrade-coverage.test.mjs scripts/upgrade-coverage-workflows.test.mjs scripts/coverage-report.test.mjs
node pnpm/esm-loader/scripts/test-ecosystem.mjs /path/to/verdaccio run packages/core/core/test packages/core/url/test --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs --cwd packages/shared /path/to/logto run src --maxWorkers=2 --configLoader native --no-cache
node pnpm/esm-loader/scripts/test-ecosystem.mjs /path/to/stimulus-components run components --maxWorkers=2 --configLoader native --no-cache
```

Each command retains a fixture and exits unsuccessfully if the loader run fails. Use its printed directory for the GVS attempt or the Svelte compiler comparison:

```sh
node pnpm/esm-loader/scripts/test-gvs-repository.mjs /retained-fixture vitest
node pnpm/esm-loader/scripts/test-gvs-repository.mjs /nextauth-fixture vitest vite unplugin-swc
node pnpm/esm-loader/scripts/test-gvs-repository.mjs /kysely-migration-fixture esbuild
node pnpm/esm-loader/scripts/test-gvs-repository.mjs /qwik-fixture vitest oxc-parser oxc-transform
node pnpm/esm-loader/scripts/test-gvs-repository.mjs /turborepo-fixture jest
node pnpm/esm-loader/scripts/test-gvs-repository.mjs /cyclejs-fixture mocha ts-node
node pnpm/esm-loader/scripts/test-gvs-repository.mjs /logto-fixture vitest @silverhand/ts-config
node pnpm/esm-loader/scripts/test-svelte-compiler.mjs /svelte-fixture
```

Before the Kysely migration GVS retry, remove the test-created `repo/test/node/dist/cts-migrations` directory inside that retained fixture. The timeout prevents its cleanup hook from running; leaving it in place makes the next setup fail with `EEXIST`.

For pnpm, create the fixture with `test-repository.mjs`, then use these explicit opt-outs:

```sh
node pnpm/esm-loader/scripts/test-repository.mjs
node pnpm/esm-loader/scripts/test-gvs-repository.mjs /pnpm-fixture jest @rushstack/worker-pool ts-jest-resolver @pnpm/nopt didyoumean2 is-windows p-limit bole split2 tempy empathic read-yaml-file
```

The GVS diagnostic needs an installed pnpm v12 CLI and a format-9 installed lockfile. It explicitly checks the supported format before mapping package contexts; it does not upgrade older lockfiles or re-resolve their dependency graphs. It preserves both workspace configuration and legacy `package.json` pnpm settings when installing the frozen selected graph. Patch entries absent from that graph are omitted from both staging configuration and lockfile. NextAuth.js's unrelated release-tool patch otherwise fails the frozen install because its pnpm 9 hash differs from the current patch hash format. Patches used by the selected graph remain intact; the pnpm parser regression still passes with its patched dependencies.

## Retained evidence

Every fixture retains `ecosystem-results.json` with the revision, command arguments, runtime, snapshot counts, and exit statuses, plus `baseline.stdout`, `baseline.stderr`, `cas.stdout`, and `cas.stderr`. GVS attempts that reach test execution write `gvs-results.json`, `gvs.stdout`, and `gvs.stderr`; the result includes every mapped package root and the retained staging installation path. The pnpm-specific fixture uses its own scenario report.

The diagnostic also retains partial output on timeout, with a null exit status, the termination signal, and the spawn error in the result. The all-CAS run has a three-minute limit; the GVS run has a two-minute limit. A terminated process may not flush its exit-time module audit, so timeout audit counts can be incomplete.

New runs also write CAS load audits as JSON lines, one URL array per exiting process or worker: `cas-loads.jsonl` for the all-CAS attempt and `suite-cas-loads.jsonl` for the latest GVS/selective attempt. Compiler comparisons write `compiler-results.json` and `compiler-cas-loads.jsonl`. The diagnostics retain failures as well as passes.

### Earlier individual-package experiment

Before the complete GVS fallback, a diagnostic copied individual package files without creating dependency trees. It passed pnpm's 51 parser tests with 71 materialized instances (656 files, 3.4 MiB), and Vue's reactivity suite with 3 tooling packages (154 files, 22.5 MiB). Vite stopped at `magic-string` after materializing 2 tooling packages; Svelte stopped at `esm-env` after 8 packages. These counts are not comparable to the full GVS dependency closures above. The commands and limitations remain documented in the [loader guide](./store-loader.md#selective-materialization-experiment).
