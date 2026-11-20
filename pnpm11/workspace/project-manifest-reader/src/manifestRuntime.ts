import { PnpmError } from '@pnpm/error'
import { convertEnginesRuntimeToDependencies } from '@pnpm/pkg-manifest.utils'
import type { EngineDependency, ProjectManifest } from '@pnpm/types'

export function convertManifestAfterRead (manifest: ProjectManifest): ProjectManifest {
  const cloned = cloneManifestForRuntimeConversion(manifest)
  convertEnginesRuntimeToDependencies(cloned, 'devEngines', 'devDependencies')
  convertEnginesRuntimeToDependencies(cloned, 'engines', 'dependencies')
  return cloned
}

export function convertManifestBeforeWrite (manifest: ProjectManifest): ProjectManifest {
  const cloned = cloneManifestForRuntimeConversion(manifest)
  convertDependenciesToEnginesRuntime(cloned, 'devDependencies', 'devEngines')
  convertDependenciesToEnginesRuntime(cloned, 'dependencies', 'engines')
  return cloned
}

function cloneManifestForRuntimeConversion (manifest: ProjectManifest): ProjectManifest {
  const cloned: ProjectManifest = { ...manifest }
  if (manifest.dependencies != null && typeof manifest.dependencies === 'object' && !Array.isArray(manifest.dependencies)) {
    cloned.dependencies = { ...manifest.dependencies }
  }
  if (manifest.devDependencies != null && typeof manifest.devDependencies === 'object' && !Array.isArray(manifest.devDependencies)) {
    cloned.devDependencies = { ...manifest.devDependencies }
  }
  if (manifest.engines != null && typeof manifest.engines === 'object' && !Array.isArray(manifest.engines)) {
    cloned.engines = { ...manifest.engines }
  }
  if (manifest.devEngines != null && typeof manifest.devEngines === 'object' && !Array.isArray(manifest.devEngines)) {
    cloned.devEngines = { ...manifest.devEngines }
  }
  return cloned
}

function convertDependenciesToEnginesRuntime (
  manifest: ProjectManifest,
  dependenciesFieldName: 'dependencies' | 'devDependencies',
  enginesFieldName: 'engines' | 'devEngines'
): void {
  const dependencies = readDependenciesField(manifest, dependenciesFieldName)
  for (const runtimeName of ['node', 'deno', 'bun']) {
    applyRuntimeConversion(manifest, enginesFieldName, dependencies, runtimeName)
  }
}

function applyRuntimeConversion (
  manifest: ProjectManifest,
  enginesFieldName: 'engines' | 'devEngines',
  dependencies: Record<string, unknown> | undefined,
  runtimeName: string
): void {
  const dep = dependencies?.[runtimeName]
  if (dependencies != null && typeof dep === 'string' && dep.startsWith('runtime:')) {
    setManagedRuntimeEntry(manifest, enginesFieldName, runtimeName, dep.slice('runtime:'.length).trim())
    delete dependencies[runtimeName]
  } else if (dep === undefined) {
    removeManagedRuntimeEntry(manifest[enginesFieldName], runtimeName)
  }
}

function setManagedRuntimeEntry (
  manifest: ProjectManifest,
  enginesFieldName: 'engines' | 'devEngines',
  runtimeName: string,
  version: string
): void {
  manifest[enginesFieldName] ??= {}
  const runtimeEntry: EngineDependency = {
    name: runtimeName,
    version,
    onFail: 'download',
  }
  const enginesField = manifest[enginesFieldName]!
  if (!enginesField.runtime) {
    enginesField.runtime = runtimeEntry
  } else if (Array.isArray(enginesField.runtime)) {
    updateRuntimeInList(enginesField.runtime, runtimeEntry)
  } else if (enginesField.runtime.name === runtimeName) {
    Object.assign(enginesField.runtime, runtimeEntry)
  } else {
    enginesField.runtime = [enginesField.runtime, runtimeEntry]
  }
}

function updateRuntimeInList (runtimes: EngineDependency[], runtimeEntry: EngineDependency): void {
  const existing = runtimes.find(({ name }) => name === runtimeEntry.name)
  if (existing) {
    Object.assign(existing, runtimeEntry)
  } else {
    runtimes.push(runtimeEntry)
  }
}

function readDependenciesField (
  manifest: ProjectManifest,
  dependenciesFieldName: 'dependencies' | 'devDependencies'
): Record<string, unknown> | undefined {
  const dependencies = manifest[dependenciesFieldName] as unknown
  if (dependencies === undefined) return undefined
  if (dependencies === null || typeof dependencies !== 'object' || Array.isArray(dependencies)) {
    throw new PnpmError('INVALID_DEPENDENCIES_FIELD', `The "${dependenciesFieldName}" field must be an object.`)
  }
  return dependencies as Record<string, unknown>
}

function removeManagedRuntimeEntry (
  enginesField: ProjectManifest['devEngines'] | ProjectManifest['engines'],
  runtimeName: string
): void {
  if (!enginesField?.runtime) return

  if (Array.isArray(enginesField.runtime)) {
    const runtimes = enginesField.runtime.filter((runtime) => !isManagedRuntimeEntry(runtime, runtimeName))
    if (runtimes.length === 0) {
      delete enginesField.runtime
    } else {
      enginesField.runtime = runtimes
    }
  } else if (isManagedRuntimeEntry(enginesField.runtime, runtimeName)) {
    delete enginesField.runtime
  }
}

function isManagedRuntimeEntry (runtime: EngineDependency, runtimeName: string): boolean {
  return runtime.name === runtimeName &&
    runtime.onFail === 'download' &&
    typeof runtime.version === 'string'
}
