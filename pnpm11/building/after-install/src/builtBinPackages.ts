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

export function getHoistedModulesDir (state: RebuildState): string {
  return state.ctx.hoistedModulesDir ?? path.join(state.ctx.virtualStoreDir, 'node_modules')
}

export function builtHoistedBinPackages (state: RebuildState): Set<string> {
  const hoistedModulesDir = getHoistedModulesDir(state)
  const packages = new Set<string>()
  for (const depPath of state.builtDepPaths) {
    for (const pkgRoot of privateHoistedPkgRoots(depPath as DepPath, state, hoistedModulesDir)) {
      packages.add(path.normalize(pkgRoot))
    }
  }
  return packages
}

export function privateHoistedPkgRoots (depPath: DepPath, state: RebuildState, hoistedModulesDir = getHoistedModulesDir(state)): string[] {
  const aliases = state.ctx.modulesFile?.hoistedDependencies[depPath] ?? {}
  return Object.entries(aliases)
    .filter(([, visibility]) => visibility === 'private')
    .map(([alias]) => safeJoinModulesDir(hoistedModulesDir, alias))
}

export function builtBinPackages (modulesDir: string, dependencies: Record<string, string>, state: RebuildState): Set<string> {
  return new Set(Object.entries(dependencies)
    .filter(([alias, reference]) => {
      const depPath = refToRelative(reference, alias)
      return depPath != null && state.builtDepPaths.has(depPath)
    })
    .map(([alias]) => path.normalize(safeJoinModulesDir(modulesDir, alias))))
}
