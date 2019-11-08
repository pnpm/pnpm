import {
  Dependencies,
} from '@pnpm/types'
import validateNpmPackageName = require('validate-npm-package-name')
import { PinnedVersion, WantedDependency } from './install/getWantedDependencies'
import guessPinnedVersionFromExistingSpec from './guessPinnedVersionFromExistingSpec'

export default function parseWantedDependencies (
  rawWantedDependencies: string[],
  opts: {
    allowNew: boolean,
    currentPrefs: Dependencies,
    defaultTag: string,
    dev: boolean,
    devDependencies: Dependencies,
    optional: boolean,
    optionalDependencies: Dependencies,
    updateWorkspaceDependencies?: boolean,
  },
): WantedDependency[] {
  return rawWantedDependencies
    .map((rawWantedDependency) => {
      const parsed = parseWantedDependency(rawWantedDependency)
      // tslint:disable:no-string-literal
      const alias = parsed['alias'] as (string | undefined)
      let pref = parsed['pref'] as (string | undefined)
      let pinnedVersion!: PinnedVersion | undefined
      // tslint:enable:no-string-literal
      if (!opts.allowNew && (!alias || !opts.currentPrefs[alias])) {
        return null
      }
      if (!pref && alias && opts.currentPrefs[alias]) {
        pref = (opts.currentPrefs[alias].startsWith('workspace:') && opts.updateWorkspaceDependencies === true)
          ? 'workspace:*' : opts.currentPrefs[alias]
        pinnedVersion = guessPinnedVersionFromExistingSpec(opts.currentPrefs[alias])
      }
      return {
        alias,
        dev: Boolean(opts.dev || alias && !!opts.devDependencies[alias]),
        optional: Boolean(opts.optional || alias && !!opts.optionalDependencies[alias]),
        pinnedVersion,
        pref: pref ?? opts.defaultTag,
        raw: rawWantedDependency,
      }
    })
    .filter((wd) => wd !== null) as WantedDependency[]
}

export function parseWantedDependency (
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
