import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'
import type { Dependencies } from '@pnpm/types'
import { whichVersionIsPinned } from '@pnpm/which-version-is-pinned'
import type {
  PinnedVersion,
  WantedDependency,
} from '@pnpm/resolve-dependencies/lib/getWantedDependencies'

export function parseWantedDependencies(
  rawWantedDependencies: string[],
  opts: {
    allowNew: boolean
    currentPrefs: Dependencies
    defaultTag: string
    dev: boolean
    devDependencies: Dependencies
    optional: boolean
    optionalDependencies: Dependencies
    overrides?: Record<string, string> | undefined
    updateWorkspaceDependencies?: boolean | undefined
    preferredSpecs?: Record<string, string> | undefined
  }
): WantedDependency[] {
  return rawWantedDependencies
    .map((rawWantedDependency: string): {
      pref: string;
      alias?: string | undefined;
      dev: boolean;
      optional: boolean;
      pinnedVersion?: PinnedVersion | undefined;
      raw: string;
    } | null => {
      const parsed = parseWantedDependency(rawWantedDependency)
      const alias = parsed.alias
      let pref = parsed.pref
      let pinnedVersion: PinnedVersion | undefined

      if (!opts.allowNew && (!alias || !opts.currentPrefs[alias])) {
        return null
      }

      if (alias && opts.currentPrefs[alias]) {
        if (!pref) {
          pref =
            opts.currentPrefs[alias].startsWith('workspace:') &&
            opts.updateWorkspaceDependencies === true
              ? 'workspace:*'
              : opts.currentPrefs[alias]
        }

        pinnedVersion = whichVersionIsPinned(opts.currentPrefs[alias])
      }

      const result = {
        alias,
        dev: Boolean(opts.dev || (alias && !!opts.devDependencies[alias])),
        optional: Boolean(
          opts.optional || (alias && !!opts.optionalDependencies[alias])
        ),
        pinnedVersion,
        raw:
          alias && opts.currentPrefs?.[alias]?.startsWith('workspace:')
            ? `${alias}@${opts.currentPrefs[alias]}`
            : rawWantedDependency,
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
    .filter(Boolean)
}
