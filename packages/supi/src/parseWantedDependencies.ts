import parseWantedDependency from '@pnpm/parse-wanted-dependency'
import { Dependencies } from '@pnpm/types'
import guessPinnedVersionFromExistingSpec from './guessPinnedVersionFromExistingSpec'
import { PinnedVersion, WantedDependency } from './install/getWantedDependencies'

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
  }
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
      if (alias && opts.currentPrefs[alias]) {
        if (!pref) {
          pref = (opts.currentPrefs[alias].startsWith('workspace:') && opts.updateWorkspaceDependencies === true)
            ? 'workspace:*' : opts.currentPrefs[alias]
        }
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
