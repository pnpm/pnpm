import {Dependencies} from '@pnpm/types'
import {WantedDependency} from './types'

export default function (
  deps: Dependencies,
  opts: {
    optionalDependencies: Dependencies,
    devDependencies: Dependencies,
  }
): WantedDependency[] {
  if (!deps) return []
  return Object.keys(deps).map(alias => ({
    alias,
    pref: deps[alias],
    dev: !!opts.devDependencies[alias],
    optional: !!opts.optionalDependencies[alias],
    raw: `${alias}@${deps[alias]}`,
  }))
}

export function similarDepsToSpecs (
  deps: Dependencies,
  opts: {
    dev: boolean,
    optional: boolean,
    optionalDependencies: Dependencies,
    devDependencies: Dependencies,
    currentPrefs: Dependencies,
  }
): WantedDependency[] {
  if (!deps) return []
  return Object.keys(deps).map(alias => ({
    alias,
    pref: deps[alias] || opts.currentPrefs[alias],
    dev: opts.dev || !!opts.devDependencies[alias],
    optional: opts.optional || !!opts.optionalDependencies[alias],
    raw: `${alias}@${deps[alias]}`,
  }))
}
