import * as dp from '@pnpm/deps.path'
import type { DepPath } from '@pnpm/types'

import type { HeadlessContext } from './context.js'

/** Includes every lockfile importer so filtered installs keep the same workspace alias conflicts. */
export function getDirectDependencyAliases ({ opts, skipped, wantedLockfile }: HeadlessContext): string[] {
  return Object.values(wantedLockfile.importers).flatMap((importer) => {
    const refs = {
      ...(opts.include.devDependencies ? importer.devDependencies : {}),
      ...(opts.include.dependencies ? importer.dependencies : {}),
      ...(opts.include.dependencies && opts.include.optionalDependencies ? importer.optionalDependencies : {}),
    }
    return Object.entries(refs)
      .filter(entry => isHoistableDirectDependencyAlias(entry, opts, skipped))
      .map(([alias]) => alias)
  })
}

function isHoistableDirectDependencyAlias (
  [alias, ref]: [string, string],
  opts: HeadlessContext['opts'],
  skipped: Set<DepPath>
): boolean {
  if (opts.skipRuntimes && ref.startsWith('runtime:')) return false
  const depPath = dp.refToRelative(ref, alias)
  return depPath != null && !skipped.has(depPath)
}
