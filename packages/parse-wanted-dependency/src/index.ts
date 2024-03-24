import '@total-typescript/ts-reset'

import validateNpmPackageName from 'validate-npm-package-name'

import type { ParseWantedDependencyResult } from '@pnpm/types'

export function parseWantedDependency(
  rawWantedDependency: string | undefined
): ParseWantedDependencyResult {
  const versionDelimiter = rawWantedDependency?.indexOf('@', 1) // starting from 1 to skip the @ that marks scope

  if (versionDelimiter !== -1) {
    const alias = rawWantedDependency?.slice(0, versionDelimiter)

    if (versionDelimiter && alias && validateNpmPackageName(alias).validForOldPackages) {
      return {
        alias,
        pref: rawWantedDependency?.slice(versionDelimiter + 1) ?? '',
      }
    }

    return {
      pref: rawWantedDependency ?? '',
    }
  }

  if (validateNpmPackageName(rawWantedDependency ?? '').validForOldPackages) {
    return {
      alias: rawWantedDependency ?? '',
    }
  }

  return {
    pref: rawWantedDependency ?? '',
  }
}
