import { isPnpmCliDependency } from '@pnpm/global.commands'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'

/**
 * Whether any of `params` names the pnpm CLI itself. Each selector is
 * normalized to the package it installs first, so neither a versioned form like
 * `pnpm@9` nor an aliased one like `foo@npm:pnpm@9` bypasses the guard.
 */
export function selectsPnpmCli (params: readonly string[]): boolean {
  return params.some((param) => {
    const { alias, bareSpecifier } = parseWantedDependency(param)
    return isPnpmCliDependency(alias ?? '', bareSpecifier)
  })
}
