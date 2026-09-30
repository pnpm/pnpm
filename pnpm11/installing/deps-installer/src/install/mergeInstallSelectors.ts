import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import type { DependenciesField, ProjectManifest } from '@pnpm/types'

import type { InstallSomeDepsMutation } from './mutationTypes.js'

const DEPENDENCY_FIELDS_TO_CLEAR: DependenciesField[] = ['dependencies', 'devDependencies', 'optionalDependencies']

/**
 * Merge `installSome` selectors into the manifest, choosing the target
 * dependency field per the mutation's `targetDependenciesField` (or the
 * existing field if the dep is already in the manifest, defaulting to
 * `dependencies`). Selectors without a version use `'latest'` so the
 * pnpr server's resolver picks the newest matching release.
 */
export function mergeInstallSelectors (manifest: ProjectManifest, mutation: InstallSomeDepsMutation): ProjectManifest {
  const target = mutation.targetDependenciesField
  for (const sel of mutation.dependencySelectors) {
    const parsed = parseWantedDependency(sel)
    if (!parsed.alias) continue
    const alias = parsed.alias
    const field: DependenciesField = target ?? guessDepField(alias, manifest) ?? 'dependencies'
    const spec = parsed.bareSpecifier ?? findExistingSpec(alias, manifest) ?? 'latest'
    manifest[field] = manifest[field] ?? {}
    manifest[field]![alias] = spec
    // If `targetDependenciesField` is set, also remove the alias from the
    // other fields — matches the normal flow's behavior.
    if (target) {
      removeFromOtherFields(manifest, { alias, target })
    }
    if (mutation.peer) {
      manifest.peerDependencies = manifest.peerDependencies ?? {}
      manifest.peerDependencies[alias] = manifest.peerDependencies[alias] ?? spec
    }
  }
  return manifest
}

function removeFromOtherFields (
  manifest: ProjectManifest,
  { alias, target }: { alias: string, target: DependenciesField }
): void {
  for (const other of DEPENDENCY_FIELDS_TO_CLEAR) {
    if (other !== target) delete manifest[other]?.[alias]
  }
}

function guessDepField (alias: string, manifest: ProjectManifest): DependenciesField | undefined {
  if (manifest.dependencies?.[alias] != null) return 'dependencies'
  if (manifest.devDependencies?.[alias] != null) return 'devDependencies'
  if (manifest.optionalDependencies?.[alias] != null) return 'optionalDependencies'
  return undefined
}

function findExistingSpec (alias: string, manifest: ProjectManifest): string | undefined {
  return manifest.dependencies?.[alias] ??
    manifest.devDependencies?.[alias] ??
    manifest.optionalDependencies?.[alias]
}
