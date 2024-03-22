import '@total-typescript/ts-reset'

import { PnpmError } from '@pnpm/error'
import type { VersionOverride } from '@pnpm/types'
import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'

const DELIMITER_REGEX = /[^ |@]>/

export function parseOverrides(
  overrides: Record<string, string>
): VersionOverride[] {
  return Object.entries(overrides).map(([selector, newPref]) => {
    let delimiterIndex = selector.search(DELIMITER_REGEX)

    if (delimiterIndex !== -1) {
      delimiterIndex++

      const parentSelector = selector.substring(0, delimiterIndex)

      const childSelector = selector.substring(delimiterIndex + 1)

      return {
        newPref,
        parentPkg: parsePkgSelector(parentSelector),
        targetPkg: parsePkgSelector(childSelector),
      }
    }

    return {
      newPref,
      targetPkg: parsePkgSelector(selector),
    }
  })
}

function parsePkgSelector(selector: string): {
  name: string;
  pref: string | undefined;
} {
  const wantedDep = parseWantedDependency(selector)

  if (!wantedDep.alias) {
    throw new PnpmError(
      'INVALID_SELECTOR',
      `Cannot parse the "${selector}" selector`
    )
  }

  return {
    name: wantedDep.alias,
    pref: wantedDep.pref,
  }
}
