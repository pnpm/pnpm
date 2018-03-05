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
    currentPrefs: Dependencies,
    defaultTag: string,
    dev: boolean,
    devDependencies: Dependencies,
    optional: boolean,
    optionalDependencies: Dependencies,
  },
): WantedDependency[] {
  return rawWantedDependencies
    .map((rawWantedDependency) => {
      const parsed = parseWantedDependency(rawWantedDependency)
      // tslint:disable:no-string-literal
      const alias = parsed['alias'] as (string | undefined)
      const pref = parsed['pref'] as (string | undefined)
      // tslint:enable:no-string-literal
      return {
        alias,
        dev: Boolean(opts.dev || alias && !!opts.devDependencies[alias]),
        optional: Boolean(opts.optional || alias && !!opts.optionalDependencies[alias]),
        pref: pref || alias && opts.currentPrefs[alias] || opts.defaultTag,
        raw: rawWantedDependency,
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
