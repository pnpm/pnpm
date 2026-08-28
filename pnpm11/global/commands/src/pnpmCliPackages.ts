import type { GlobalPackageInfo } from '@pnpm/global.packages'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'

/**
 * The wrapper packages that ship the pnpm CLI itself: the unscoped `pnpm` and
 * the `@pnpm/exe` native build.
 *
 * `pnpm self-update` owns their global install — it is what points the pnpm
 * home's bins at a particular release. The ordinary global commands therefore
 * keep away from them: `pnpm add -g` refuses to create such an install, and
 * `pnpm update -g` leaves an existing one alone. Otherwise a global update
 * would resolve the pnpm CLI from a dist-tag of its own choosing and relink the
 * bins, swapping the running pnpm for another version behind the user's back.
 */
const PNPM_CLI_PACKAGE_NAMES: ReadonlySet<string> = new Set(['pnpm', '@pnpm/exe'])

/**
 * Whether `pkg` is a global group the pnpm CLI is installed in. `update -g`
 * leaves the whole group alone: reinstalling it would relink pnpm's bin
 * whatever else the group holds.
 */
export function hasPnpmCliDependency (pkg: Pick<GlobalPackageInfo, 'dependencies'>): boolean {
  return Object.entries(pkg.dependencies).some(([alias, spec]) => isPnpmCliDependency(alias, spec))
}

/** Whether `pkg` is a global group holding nothing but the pnpm CLI. */
export function isPnpmCliOnlyGroup (pkg: Pick<GlobalPackageInfo, 'dependencies'>): boolean {
  const deps = Object.entries(pkg.dependencies)
  return deps.length > 0 && deps.every(([alias, spec]) => isPnpmCliDependency(alias, spec))
}

/**
 * Whether any of `selectors` names the pnpm CLI. Each is normalized to the
 * package it installs first, so neither a versioned form like `pnpm@9` nor an
 * aliased one like `foo@npm:pnpm@9` bypasses the guard.
 */
export function selectsPnpmCli (selectors: readonly string[]): boolean {
  return selectors.some((selector) => {
    const { alias, bareSpecifier } = parseWantedDependency(selector)
    return isPnpmCliDependency(alias ?? '', bareSpecifier)
  })
}

/**
 * Whether a dependency declared as `alias` at `spec` is the pnpm CLI. An `npm:`
 * alias resolves to its target, so `foo` at `npm:pnpm@9` is the pnpm CLI under
 * another name — the install still carries pnpm's own `pnpm` bin.
 */
export function isPnpmCliDependency (alias: string, spec?: string): boolean {
  return PNPM_CLI_PACKAGE_NAMES.has(npmAliasTarget(spec) ?? alias)
}

/** The package an `npm:` alias points at, or `undefined` for any other spec. */
function npmAliasTarget (spec?: string): string | undefined {
  if (spec?.startsWith('npm:') !== true) return undefined
  return parseWantedDependency(spec.slice('npm:'.length)).alias
}
