import type { DependencyManifest } from '@pnpm/types'

import { getAllDependenciesFromManifest } from './getAllDependenciesFromManifest.js'

export function getAllUniqueSpecs (manifests: DependencyManifest[]): Record<string, string> {
  const allSpecs = new Map<string, string>()
  const ignored = new Set<string>()
  for (const manifest of manifests) {
    const specs = getAllDependenciesFromManifest(manifest)
    for (const [name, spec] of Object.entries(specs)) {
      if (ignored.has(name)) continue
      if (isConflictingOrProtocolSpec(allSpecs.get(name), spec)) {
        ignored.add(name)
        allSpecs.delete(name)
        continue
      }
      allSpecs.set(name, spec)
    }
  }
  return Object.fromEntries(allSpecs)
}

function isConflictingOrProtocolSpec (previousSpec: string | undefined, spec: string): boolean {
  return previousSpec != null && previousSpec !== spec || spec.includes(':')
}
