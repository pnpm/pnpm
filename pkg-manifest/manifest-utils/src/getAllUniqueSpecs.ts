import { type DependencyManifest } from '@pnpm/types'
import { getAllDependenciesFromManifest } from './getAllDependenciesFromManifest'

export function getAllUniqueSpecs (manifests: DependencyManifest[]): Record<string, string> {
  const allSpecs: Record<string, string> = {}
  const ignored = new Set<string>()
  for (const manifest of manifests) {
    const specs = getAllDependenciesFromManifest(manifest)
    for (const [name, spec] of Object.entries(specs)) {
      if (ignored.has(name)) continue
      if (allSpecs[name] != null && allSpecs[name] !== spec || spec.includes(':')) {
        ignored.add(name)
        delete allSpecs[name]
        continue
      }
      allSpecs[name] = spec
    }
  }
  return allSpecs
}
