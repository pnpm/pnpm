import type { GlobalPackageInfo } from '@pnpm/global.packages'

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
const PNPM_CLI_PACKAGE_ALIASES: ReadonlySet<string> = new Set(['pnpm', '@pnpm/exe'])

export function isPnpmCliPackageAlias (alias: string): boolean {
  return PNPM_CLI_PACKAGE_ALIASES.has(alias)
}

/** Whether `pkg` is a global group holding nothing but the pnpm CLI. */
export function isPnpmCliOnlyGroup (pkg: Pick<GlobalPackageInfo, 'dependencies'>): boolean {
  const aliases = Object.keys(pkg.dependencies)
  return aliases.length > 0 && aliases.every(isPnpmCliPackageAlias)
}
