// Applies the pending release plan, then runs the meta-updater to mirror the
// bumped Rust wrapper versions into the Rust sources the release builds from.
//
// `pnpm version -r` (native workspace release management) consumes the pending
// `.changeset/*.md` intents: it bumps versions across the workspace, writes
// changelogs, and records consumed intents in the committed `.changeset/
// ledger.yaml`. The ledger keeps cherry-picks and merge-backs between release
// branches safe, and the Rust products' `alpha` release lanes (configured under
// `versioning` in pnpm-workspace.yaml) advance their `X.Y.Z-alpha.N` prerelease
// lines. `pnpm version -r` bumps only the npm wrapper manifests, so the
// meta-updater then copies those versions into the Rust sources that embed
// them (see the Rust-source handlers in `.meta-updater/src/index.ts`);
// `meta-updater --test` in pre-push and CI enforces the same sync.
//
// `--release <product>` (repeatable) restricts the run to a subset of the three
// releasable products, so a frequent v12 (Rust) release no longer has to drag
// the TypeScript CLI (v11) along. With no `--release` flag every pending intent
// is consumed, so a bare `pnpm bump` still cuts a full release.

import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import url from 'node:url'

// The workspace package selectors for each alpha-lane product, keyed by the
// product's release token. These mirror `versioning.lanes` in
// pnpm-workspace.yaml. The `pnpm` product is the Rust CLI (published to npm as
// `pnpm`, named `pacquet` in-repo, carrying its versioning.fixed `@pnpm/napi`
// sibling). The `pnpm11` product — the TypeScript CLI and the rest of the
// default lane — has no single positive selector, so it is expressed as the
// complement of the alpha products (see releaseFilterArgs).
const ALPHA_PRODUCT_PACKAGES = {
  pnpm: ['pacquet', '@pnpm/napi'],
  pnpr: ['@pnpm/pnpr'],
} as const

type AlphaProduct = keyof typeof ALPHA_PRODUCT_PACKAGES
type Product = 'pnpm11' | AlphaProduct

const PRODUCTS: readonly Product[] = ['pnpm11', 'pnpm', 'pnpr']

// The module-level consts are still in their temporal dead zone while this
// file's statements run, so the actual `main()` call sits at the bottom.
function main (): void {
  const repoRoot = findRepoRoot(import.meta.dirname)
  const filterArgs = releaseFilterArgs(parseSelectedProducts(process.argv.slice(2)))
  // The release PR branch is dirty here (refreshed trust roots, synthesized
  // changesets), so skip the clean-tree check. Pass the arguments as an argv
  // array (no shell) so a filter value is never interpreted by a shell.
  execFileSync('pnpm', ['version', '-r', '--no-git-checks', ...filterArgs], { cwd: repoRoot, stdio: 'inherit' })
  execFileSync('pnpm', ['update-manifests'], { cwd: repoRoot, stdio: 'inherit' })
}

export function parseSelectedProducts (argv: readonly string[]): Set<Product> {
  const selected = new Set<Product>()
  for (let i = 0; i < argv.length; i++) {
    // Fail closed: an unrecognized token (e.g. a `--releases` typo) must not be
    // silently skipped, which would leave the selection empty and release
    // every product. Only "--release <product>" is accepted; no args at all
    // still means a full release (see releaseFilterArgs).
    if (argv[i] !== '--release') {
      throw new Error(`Unexpected bump argument: ${String(argv[i])}. Only "--release <product>" is supported.`)
    }
    const product = argv[++i]
    if (product === undefined || !(PRODUCTS as readonly string[]).includes(product)) {
      throw new Error(`Unknown --release product: ${String(product)}. Expected one of ${PRODUCTS.join(', ')}.`)
    }
    selected.add(product as Product)
  }
  return selected
}

// Turns the selected products into `--filter` arguments for `pnpm version -r`.
// An empty selection releases everything (no filter). When `pnpm11` is selected
// the run starts from the whole workspace and excludes only the alpha products
// left unselected (an exclude-only filter selects "every project minus these"),
// so selecting all three yields no filter — a full release. When `pnpm11` is not
// selected only the chosen alpha products' packages are included.
export function releaseFilterArgs (selected: ReadonlySet<Product>): string[] {
  if (selected.size === 0) return []
  const alphaProducts = Object.keys(ALPHA_PRODUCT_PACKAGES) as AlphaProduct[]
  if (selected.has('pnpm11')) {
    return alphaProducts
      .filter((product) => !selected.has(product))
      .flatMap((product) => ALPHA_PRODUCT_PACKAGES[product])
      .map((pkg) => `--filter=!${pkg}`)
  }
  return alphaProducts
    .filter((product) => selected.has(product))
    .flatMap((product) => ALPHA_PRODUCT_PACKAGES[product])
    .map((pkg) => `--filter=${pkg}`)
}

export function findRepoRoot (startDir: string): string {
  let dir = startDir
  while (!fs.existsSync(path.join(dir, '.changeset'))) {
    const parent = path.dirname(dir)
    if (parent === dir) {
      throw new Error(`No .changeset directory found above ${startDir}`)
    }
    dir = parent
  }
  return dir
}

function isDirectInvocation (): boolean {
  if (process.argv[1] === undefined) return false
  try {
    return import.meta.url === url.pathToFileURL(fs.realpathSync(process.argv[1])).href
  } catch {
    return false
  }
}

if (isDirectInvocation()) {
  main()
}
