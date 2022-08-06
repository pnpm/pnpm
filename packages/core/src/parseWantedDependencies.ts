import parseWantedDependency from '@pnpm/parse-wanted-dependency'
import { Dependencies } from '@pnpm/types'
import whichVersionIsPinned from '@pnpm/which-version-is-pinned'
import { PinnedVersion, WantedDependency } from '@pnpm/resolve-dependencies/lib/getWantedDependencies'

export default function parseWantedDependencies (
  rawWantedDependencies: string[],
  opts: {
    allowNew: boolean
    currentPrefs: Dependencies
    defaultTag: string
    dev: boolean
    devDependencies: Dependencies
    optional: boolean
    optionalDependencies: Dependencies
    overrides?: Record<string, string>
    updateWorkspaceDependencies?: boolean
    preferredSpecs?: Record<string, string>
  }
): WantedDependency[] {
  return rawWantedDependencies
    .map((rawWantedDependency) => {
      const parsed = parseWantedDependency(rawWantedDependency)
      const alias = parsed['alias']
      let pref = parsed['pref']
      let pinnedVersion!: PinnedVersion | undefined
      /* eslint-enable @typescript-eslint/dot-notation */
      if (!opts.allowNew && (!alias || !opts.currentPrefs[alias])) {
        return null
      }
      if (alias && opts.currentPrefs[alias]) {
        if (!pref) {
          pref = (opts.currentPrefs[alias].startsWith('workspace:') && opts.updateWorkspaceDependencies === true)
            ? 'workspace:*'
            : opts.currentPrefs[alias]
        }
        pinnedVersion = whichVersionIsPinned(opts.currentPrefs[alias])
      }
      const result = {
        alias,
        dev: Boolean(opts.dev || alias && !!opts.devDependencies[alias]),
        optional: Boolean(opts.optional || alias && !!opts.optionalDependencies[alias]),
        pinnedVersion,
        raw: rawWantedDependency,
      }
      if (pref) {
        return {
          ...result,
          pref,
        }
      }
      if (alias && opts.preferredSpecs?.[alias]) {
        return {
          ...result,
          pref: opts.preferredSpecs[alias],
          raw: `${rawWantedDependency}@${opts.preferredSpecs[alias]}`,
        }
      }
      if (alias && opts.overrides?.[alias]) {
        return {
          ...result,
          pref: opts.overrides[alias],
          raw: `${alias}@${opts.overrides[alias]}`,
        }
      }
      return {
        ...result,
        pref: opts.defaultTag,
      }
    })
    .filter((wd) => wd !== null) as WantedDependency[]
}
