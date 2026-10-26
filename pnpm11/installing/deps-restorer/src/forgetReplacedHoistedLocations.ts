import path from 'node:path'

import type { DependenciesGraph } from '@pnpm/deps.graph-builder'
import { writeModulesManifest } from '@pnpm/installing.modules-yaml'

import type { HeadlessContext } from './context.js'

/**
 * Removes from `.modules.yaml` every recorded location this install does not
 * reuse as it is, before the linker replaces or removes it. The next install
 * trusts a recorded location, so an install interrupted after this point must
 * not leave a record of the content it was replacing. Writes nothing when every
 * recorded location is reused.
 */
export async function forgetReplacedHoistedLocations ({ opts, rootModulesDir }: HeadlessContext, graph: DependenciesGraph): Promise<void> {
  const recorded = opts.modulesFile?.hoistedLocations
  if (recorded == null) return
  const isReused = (depPath: string, location: string): boolean => {
    const node = graph[path.join(opts.lockfileDir, location)]
    return node != null && node.fetching == null && node.depPath === depPath
  }
  const kept: Record<string, string[]> = {}
  let droppedAny = false
  for (const [depPath, locations] of Object.entries(recorded)) {
    const reused = locations.filter((location) => isReused(depPath, location))
    if (reused.length !== locations.length) droppedAny = true
    if (reused.length > 0) kept[depPath] = reused
  }
  if (!droppedAny) return
  await writeModulesManifest(rootModulesDir, { ...opts.modulesFile!, hoistedLocations: kept })
}
