import path from 'node:path'

import { createBinRefreshPlan } from '@pnpm/bins.linker'

import type { RebuildState } from './rebuildTypes.js'

const refreshPlans = new WeakMap<RebuildState, Map<string, ReturnType<typeof createBinRefreshPlan>>>()

const pendingLinks = new Map<string, Promise<void>>()

export async function relinkHoistedPackageBins (pkgRoots: string[], state: RebuildState): Promise<void> {
  const modulesDirs = new Set(pkgRoots.map(containingModulesDir))
  await Promise.all(Array.from(modulesDirs, async modulesDir => {
    const forceForPackages = new Set(pkgRoots.filter(pkgRoot => containingModulesDir(pkgRoot) === modulesDir).map(pkgRoot => path.normalize(pkgRoot)))
    const previous = pendingLinks.get(modulesDir) ?? Promise.resolve()
    const current = previous.then(async () => {
      const refresh = await getRefreshPlan(modulesDir, state)
      await refresh(forceForPackages)
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

function getRefreshPlan (modulesDir: string, state: RebuildState): ReturnType<typeof createBinRefreshPlan> {
  let plans = refreshPlans.get(state)
  if (plans == null) {
    plans = new Map()
    refreshPlans.set(state, plans)
  }
  let plan = plans.get(modulesDir)
  if (plan == null) {
    plan = createBinRefreshPlan(modulesDir, path.join(modulesDir, '.bin'), { warn: state.warn })
    plans.set(modulesDir, plan)
  }
  return plan
}
