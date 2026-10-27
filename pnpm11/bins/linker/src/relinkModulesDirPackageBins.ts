import path from 'node:path'

import { createBinRefreshPlan, type LinkBinOptions, type WarnFunction } from './index.js'

const refreshPlans = new WeakMap<object, Map<string, ReturnType<typeof createBinRefreshPlan>>>()
const pendingLinks = new Map<string, Promise<void>>()

export async function relinkModulesDirPackageBins (
  pkgRoots: string[],
  state: object,
  opts: LinkBinOptions & { warn: WarnFunction }
): Promise<void> {
  const modulesDirs = new Set(pkgRoots.map(containingModulesDir))
  await Promise.all(Array.from(modulesDirs, async modulesDir => {
    const forceForPackages = new Set(pkgRoots.filter(pkgRoot => containingModulesDir(pkgRoot) === modulesDir).map(pkgRoot => path.normalize(pkgRoot)))
    const previous = pendingLinks.get(modulesDir) ?? Promise.resolve()
    const current = previous.catch(() => {}).then(async () => {
      const refresh = await getRefreshPlan(modulesDir, state, opts)
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

function getRefreshPlan (
  modulesDir: string,
  state: object,
  opts: LinkBinOptions & { warn: WarnFunction }
): ReturnType<typeof createBinRefreshPlan> {
  let plans = refreshPlans.get(state)
  if (plans == null) {
    plans = new Map()
    refreshPlans.set(state, plans)
  }
  let plan = plans.get(modulesDir)
  if (plan == null) {
    plan = createBinRefreshPlan(modulesDir, path.join(modulesDir, '.bin'), opts).catch((err: unknown) => {
      plans?.delete(modulesDir)
      throw err
    })
    plans.set(modulesDir, plan)
  }
  return plan
}
