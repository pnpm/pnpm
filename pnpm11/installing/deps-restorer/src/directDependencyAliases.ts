import * as dp from '@pnpm/deps.path'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { DepPath, IncludedDependencies } from '@pnpm/types'

export interface DirectDependencyAliasesOptions {
  include: IncludedDependencies
  skipped: Set<DepPath>
  skipRuntimes?: boolean
  wantedLockfile: LockfileObject
}

/** Includes every lockfile importer so filtered installs keep the same workspace alias conflicts. */
export function getDirectDependencyAliases (opts: DirectDependencyAliasesOptions): string[] {
  return Object.values(opts.wantedLockfile.importers).flatMap((importer) => {
    const refs = {
      ...(opts.include.devDependencies ? importer.devDependencies : {}),
      ...(opts.include.dependencies ? importer.dependencies : {}),
      ...(opts.include.dependencies && opts.include.optionalDependencies ? importer.optionalDependencies : {}),
    }
    return Object.entries(refs)
      .filter(entry => isHoistableDirectDependencyAlias(entry, opts))
      .map(([alias]) => alias)
  })
}

function isHoistableDirectDependencyAlias (
  [alias, ref]: [string, string],
  opts: DirectDependencyAliasesOptions
): boolean {
  if (opts.skipRuntimes && ref.startsWith('runtime:')) return false
  const depPath = dp.refToRelative(ref, alias)
  return depPath != null && !opts.skipped.has(depPath)
}
