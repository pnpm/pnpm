import {Dependencies} from '@pnpm/types'
import {WantedDependency} from './types'

export default function (
  deps: Dependencies,
  opts: {
    devDependencies: Dependencies,
    optionalDependencies: Dependencies,
  },
): WantedDependency[] {
  if (!deps) return []
  return Object.keys(deps).map((alias) => ({
    alias,
    dev: !!opts.devDependencies[alias],
    optional: !!opts.optionalDependencies[alias],
    pref: deps[alias],
    raw: `${alias}@${deps[alias]}`,
  }))
}

export function similarDepsToSpecs (
  deps: Dependencies,
  opts: {
    currentPrefs: Dependencies,
    dev: boolean,
    devDependencies: Dependencies,
    optional: boolean,
    optionalDependencies: Dependencies,
  },
): WantedDependency[] {
  if (!deps) return []
  return Object.keys(deps).map((alias) => ({
    alias,
    dev: opts.dev || !!opts.devDependencies[alias],
    optional: opts.optional || !!opts.optionalDependencies[alias],
    pref: deps[alias] || opts.currentPrefs[alias],
    raw: `${alias}@${deps[alias]}`,
  }))
}
