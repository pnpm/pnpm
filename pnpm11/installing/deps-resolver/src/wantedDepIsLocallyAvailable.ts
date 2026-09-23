import { parseBareSpecifier, pickMatchingLocalVersionOrNull } from '@pnpm/resolving.npm-resolver'
import type { WorkspacePackages } from '@pnpm/resolving.resolver-base'

import type { WantedDependency } from './getNonDevWantedDependencies.js'

export function wantedDepIsLocallyAvailable (
  workspacePackages: WorkspacePackages,
  wantedDependency: WantedDependency,
  opts: {
    defaultTag: string
    registry: string
  }
): boolean {
  const spec = parseBareSpecifier(wantedDependency.bareSpecifier, wantedDependency.alias, opts.defaultTag || 'latest', opts.registry)
  if ((spec == null) || !workspacePackages.has(spec.name)) return false
  return pickMatchingLocalVersionOrNull(workspacePackages.get(spec.name)!, spec) !== null
}

