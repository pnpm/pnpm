import type { ProjectSnapshot, ResolvedDependencies } from '@pnpm/lockfile.types'
import type { DependenciesField } from '@pnpm/types'

export function filterImporter (
  importer: ProjectSnapshot,
  include: { [dependenciesField in DependenciesField]: boolean },
  opts?: { skipRuntimes?: boolean }
): ProjectSnapshot {
  const skipRuntimes = opts?.skipRuntimes === true
  return {
    dependencies: !include.dependencies ? {} : pickNonRuntime(importer.dependencies, skipRuntimes),
    devDependencies: !include.devDependencies ? {} : pickNonRuntime(importer.devDependencies, skipRuntimes),
    optionalDependencies: !include.optionalDependencies ? {} : pickNonRuntime(importer.optionalDependencies, skipRuntimes),
    specifiers: pickNonRuntime(importer.specifiers, skipRuntimes),
  }
}

function pickNonRuntime (deps: ResolvedDependencies | undefined, skipRuntimes: boolean): ResolvedDependencies {
  if (!deps) return {}
  if (!skipRuntimes) return deps
  const result: ResolvedDependencies = {}
  for (const [name, ref] of Object.entries(deps)) {
    if (!ref.startsWith('runtime:')) {
      result[name] = ref
    }
  }
  return result
}
