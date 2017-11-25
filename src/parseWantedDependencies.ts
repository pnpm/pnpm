import {
  Dependencies,
} from '@pnpm/types'
import validateNpmPackageName = require('validate-npm-package-name')
import {
  WantedDependency,
} from './types'

export default function parseWantedDependencies (
  rawWantedDependencies: string[],
  opts: {
    defaultTag: string,
    dev: boolean,
    optional: boolean,
    currentPrefs: Dependencies,
    optionalDependencies: Dependencies,
    devDependencies: Dependencies,
  }
): WantedDependency[] {
  return rawWantedDependencies
    .map(rawWantedDependency => {
      const parsed = parseWantedDependency(rawWantedDependency)
      const alias = parsed['alias'] as (string | undefined)
      const pref = parsed['pref'] as (string | undefined)
      return {
        alias,
        raw: rawWantedDependency,
        pref: pref || alias && opts.currentPrefs[alias] || opts.defaultTag,
        dev: Boolean(opts.dev || alias && !!opts.devDependencies[alias]),
        optional: Boolean(opts.optional || alias && !!opts.optionalDependencies[alias]),
      }
    })
}

function parseWantedDependency (
  rawWantedDependency: string,
): {alias: string} | {pref: string} | {alias: string, pref: string} {
  const versionDelimiter = rawWantedDependency.indexOf('@', 1) // starting from 1 to skip the @ that marks scope
  if (versionDelimiter !== -1) {
    const alias = rawWantedDependency.substr(0, versionDelimiter)
    if (validateNpmPackageName(alias).validForOldPackages) {
      return {
        alias,
        pref: rawWantedDependency.substr(versionDelimiter + 1),
      }
    }
    return {
      pref: rawWantedDependency,
    }
  }
  if (validateNpmPackageName(rawWantedDependency).validForOldPackages) {
    return {
      alias: rawWantedDependency,
    }
  }
  return {
    pref: rawWantedDependency,
  }
}
