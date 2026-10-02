import path from 'node:path'

import { refToRelative } from '@pnpm/deps.path'
import { safeJoinModulesDir } from '@pnpm/fs.symlink-dependency'
import type { DepPath } from '@pnpm/types'

import type { RebuildState } from './rebuildTypes.js'

export function builtProjectBinPackages (modulesDir: string, dependencies: Record<string, string>, state: RebuildState): Set<string> {
  const packages = builtBinPackages(modulesDir, dependencies, state)
  if (path.normalize(modulesDir) !== path.normalize(state.ctx.rootModulesDir)) return packages
  for (const depPath of state.builtDepPaths) {
    const aliases = state.ctx.modulesFile?.hoistedDependencies[depPath as DepPath] ?? {}
    for (const [alias, visibility] of Object.entries(aliases)) {
      if (visibility === 'public') packages.add(path.normalize(safeJoinModulesDir(modulesDir, alias)))
    }
  }
  return packages
}

export function builtBinPackages (modulesDir: string, dependencies: Record<string, string>, state: RebuildState): Set<string> {
  return new Set(Object.entries(dependencies)
    .filter(([alias, reference]) => {
      const depPath = refToRelative(reference, alias)
      return depPath != null && state.builtDepPaths.has(depPath)
    })
    .map(([alias]) => path.normalize(safeJoinModulesDir(modulesDir, alias))))
}
