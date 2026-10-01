import path from 'node:path'

import { linkBins } from '@pnpm/bins.linker'

import type { RebuildState } from './rebuildTypes.js'

const pendingLinks = new Map<string, Promise<void>>()

export async function relinkHoistedPackageBins (pkgRoots: string[], state: RebuildState): Promise<void> {
  const forceForPackages = new Set(pkgRoots.map(pkgRoot => path.normalize(pkgRoot)))
  const modulesDirs = new Set(pkgRoots.map(containingModulesDir))
  await Promise.all(Array.from(modulesDirs, async modulesDir => {
    const previous = pendingLinks.get(modulesDir) ?? Promise.resolve()
    const current = previous.then(async () => {
      await linkBins(modulesDir, path.join(modulesDir, '.bin'), { forceForPackages, warn: state.warn })
    })
    pendingLinks.set(modulesDir, current)
    try {
      await current
    } finally {
      if (pendingLinks.get(modulesDir) === current) pendingLinks.delete(modulesDir)
    }
  }))
}

function containingModulesDir (pkgRoot: string): string {
  const parent = path.dirname(pkgRoot)
  return path.basename(parent).startsWith('@') ? path.dirname(parent) : parent
}
