import { isPnpmCliPackageAlias } from '@pnpm/global.commands'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'

/**
 * Whether any of `params` names the pnpm CLI itself. Each selector is
 * normalized to its package name first, so versioned forms like `pnpm@9` or
 * `@pnpm/exe@1` can't bypass the guard.
 */
export function selectsPnpmCli (params: readonly string[]): boolean {
  return params.some((param) => {
    const { alias } = parseWantedDependency(param)
    return alias != null && isPnpmCliPackageAlias(alias)
  })
}
