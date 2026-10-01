---
title: Store loader compatibility results
---

These are scoped experiments with the [experimental store loader](./store-loader.md), not full repository test passes. The external projects come from [Workspaces in the wild](https://pnpm.io/workspaces#workspaces-in-the-wild). Results were collected on Linux x64 with Node.js 26.10.0. The latest batch, recorded on October 1, 2026, adds SvelteKit, Slidev, NextAuth.js, and Rollup plugins.

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

### What the passing runs establish

Astro's selected tests exercise compiled workspace code with Node's test runner and no materialized registry packages. Only four registry modules were observed through the CAS hooks. The snapshot contains 1,951 registry package instances and 565 workspace roots, so this is deliberately narrow coverage of that graph.

The Svelte comparison compiles 100 real repository components for both client and server. Hashes of generated JavaScript, CSS, source maps, and warnings match the baseline. This bypasses Vitest and does not establish that Svelte's test runner works with all-CAS dependencies.

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

The GVS fallback addresses physical-file requirements within an installed dependency tree. It does not make a separate resolver understand application dependencies in the loader manifest. These workloads need resolver integration or additional physical application links, not just more copies of package files.

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
```

For Mermaid, add a diagnostic-only `compatibility.workspace.json` containing `["./vite.config.ts"]`. This selects the existing root configuration without loading the unrelated documentation configuration. The latter uses `__dirname`, which fails under the config runner. Mermaid needs `--configLoader runner` because its configuration imports TypeScript sources through `.js` specifiers. No upstream test sources were changed.

For SvelteKit, add `packages/kit/compatibility.config.js` to redirect the cache while preserving its existing test configuration:

```js
import config from './vitest.kit.config.js';
export default { ...config, cacheDir: '.cache/loader-compatibility' };
```

The snapshot excludes `.cache`, so the loader run does not reuse the baseline's generated dependency cache. The NextAuth.js probe disables coverage for both baseline and loader runs. Its core build supplies the generated `jwt.js` imported by the existing JWT tests. The Rollup plugin-utils tests likewise require built outputs. The diagnostic's `--cwd` option runs package-specific configurations from the same relative directory in the baseline and copied checkout; it still snapshots the whole workspace.

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
```

Each command retains a fixture and exits unsuccessfully if the loader run fails. Use its printed directory for the GVS attempt or the Svelte compiler comparison:

```sh
node pnpm/esm-loader/scripts/test-gvs-repository.mjs /retained-fixture vitest
node pnpm/esm-loader/scripts/test-gvs-repository.mjs /nextauth-fixture vitest vite unplugin-swc
node pnpm/esm-loader/scripts/test-svelte-compiler.mjs /svelte-fixture
```

For pnpm, create the fixture with `test-repository.mjs`, then use these explicit opt-outs:

```sh
node pnpm/esm-loader/scripts/test-repository.mjs
node pnpm/esm-loader/scripts/test-gvs-repository.mjs /pnpm-fixture jest @rushstack/worker-pool ts-jest-resolver @pnpm/nopt didyoumean2 is-windows p-limit bole split2 tempy empathic read-yaml-file
```

The GVS diagnostic needs an installed pnpm v12 CLI. It preserves both workspace configuration and legacy `package.json` pnpm settings when installing the frozen selected graph. Patch entries absent from that graph are omitted from both staging configuration and lockfile. NextAuth.js's unrelated release-tool patch otherwise fails the frozen install because its pnpm 9 hash differs from the current patch hash format. Patches used by the selected graph remain intact; the pnpm parser regression still passes with its patched dependencies.

## Retained evidence

Every fixture retains `ecosystem-results.json` with the revision, command arguments, runtime, snapshot counts, and exit statuses, plus `baseline.stdout`, `baseline.stderr`, `cas.stdout`, and `cas.stderr`. GVS attempts write `gvs-results.json`, `gvs.stdout`, and `gvs.stderr`; the result includes every mapped package root and the retained staging installation path. The pnpm-specific fixture uses its own scenario report.

New runs also write CAS load audits as JSON lines, one URL array per exiting process or worker: `cas-loads.jsonl` for the all-CAS attempt and `suite-cas-loads.jsonl` for the latest GVS/selective attempt. Compiler comparisons write `compiler-results.json` and `compiler-cas-loads.jsonl`. The diagnostics retain failures as well as passes.

### Earlier individual-package experiment

Before the complete GVS fallback, a diagnostic copied individual package files without creating dependency trees. It passed pnpm's 51 parser tests with 71 materialized instances (656 files, 3.4 MiB), and Vue's reactivity suite with 3 tooling packages (154 files, 22.5 MiB). Vite stopped at `magic-string` after materializing 2 tooling packages; Svelte stopped at `esm-env` after 8 packages. These counts are not comparable to the full GVS dependency closures above. The commands and limitations remain documented in the [loader guide](./store-loader.md#selective-materialization-experiment).
