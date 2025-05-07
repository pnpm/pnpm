import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'
import { type Dependencies } from '@pnpm/types'
import { type WantedDependency } from '@pnpm/resolve-dependencies'
import { type Catalog } from '@pnpm/catalogs.types'

export function parseWantedDependencies (
  rawWantedDependencies: string[],
  opts: {
    allowNew: boolean
    currentBareSpecifiers: Dependencies
    defaultTag: string
    dev: boolean
    devDependencies: Dependencies
    optional: boolean
    optionalDependencies: Dependencies
    overrides?: Record<string, string>
    updateWorkspaceDependencies?: boolean
    preferredSpecs?: Record<string, string>
    defaultCatalog?: Catalog
  }
): WantedDependency[] {
  return rawWantedDependencies
    .map((rawWantedDependency) => {
      const parsed = parseWantedDependency(rawWantedDependency)
      const alias = parsed['alias']
      let bareSpecifier = parsed['bareSpecifier']

      if (!opts.allowNew && (!alias || !opts.currentBareSpecifiers[alias])) {
        return null
      }
      if (alias && opts.defaultCatalog?.[alias] && (
        (!opts.currentBareSpecifiers[alias] && bareSpecifier === undefined) ||
          opts.defaultCatalog[alias] === bareSpecifier ||
          opts.defaultCatalog[alias] === opts.currentBareSpecifiers[alias]
      )) {
        bareSpecifier = 'catalog:'
      }
      if (alias && opts.currentBareSpecifiers[alias]) {
        bareSpecifier ??= opts.currentBareSpecifiers[alias]
      }
      const result = {
        alias,
        dev: Boolean(opts.dev || alias && !!opts.devDependencies[alias]),
        optional: Boolean(opts.optional || alias && !!opts.optionalDependencies[alias]),
        prevSpecifier: alias && opts.currentBareSpecifiers[alias],
      }
      if (bareSpecifier) {
        return {
          ...result,
          bareSpecifier,
        }
      }
      if (alias && opts.preferredSpecs?.[alias]) {
        return {
          ...result,
          bareSpecifier: opts.preferredSpecs[alias],
        }
      }
      if (alias && opts.overrides?.[alias]) {
        return {
          ...result,
          bareSpecifier: opts.overrides[alias],
        }
      }
      return {
        ...result,
        bareSpecifier: opts.defaultTag,
      }
    })
    .filter((wd) => wd !== null) as WantedDependency[]
}
