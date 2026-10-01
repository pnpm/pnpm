import { globalWarn } from '@pnpm/logger'
import {
  type DependenciesField,
  type EngineDependency,
  type ProjectManifest,
  RUNTIME_NAMES,
} from '@pnpm/types'

export function convertEnginesRuntimeToDependencies (
  manifest: ProjectManifest,
  enginesFieldName: 'devEngines' | 'engines',
  dependenciesFieldName: DependenciesField
): void {
  for (const runtimeName of RUNTIME_NAMES) {
    const runtimes = toRuntimeList(manifest[enginesFieldName]?.runtime)
    if (runtimes == null || manifest[dependenciesFieldName]?.[runtimeName]) {
      continue
    }
    const runtime = runtimes.find((runtime) => runtime.name === runtimeName)
    if (runtime?.onFail !== 'download') {
      continue
    }
    if (typeof runtime.version !== 'string') {
      globalWarn(`Cannot download ${runtimeName} because no version is specified in ${enginesFieldName}.runtime`)
      continue
    }
    addRuntimeDependency(manifest, dependenciesFieldName, { runtimeName, version: runtime.version.trim() })
  }
}

function addRuntimeDependency (
  manifest: ProjectManifest,
  dependenciesFieldName: DependenciesField,
  { runtimeName, version }: { runtimeName: string, version: string }
): void {
  if ('webcontainer' in process.versions) {
    globalWarn(`Installation of ${runtimeName} versions is not supported in WebContainer`)
    return
  }
  const deps = (manifest[dependenciesFieldName] ??= {})
  // Use Object.defineProperty so a future RUNTIME_NAMES entry that
  // happens to match an inherited property name (`__proto__`,
  // `constructor`, `prototype`) becomes a regular own data property
  // instead of altering Object.prototype.
  Object.defineProperty(deps, runtimeName, {
    value: `runtime:${version}`,
    enumerable: true,
    writable: true,
    configurable: true,
  })
}

export function applyRuntimeOnFailOverride (
  manifest: ProjectManifest,
  onFailOverride: 'ignore' | 'warn' | 'error' | 'download'
): void {
  for (const [enginesFieldName, dependenciesFieldName] of [
    ['devEngines', 'devDependencies'],
    ['engines', 'dependencies'],
  ] as const) {
    const runtimes = toRuntimeList(manifest[enginesFieldName]?.runtime)
    if (runtimes == null) continue
    for (const runtime of runtimes) {
      runtime.onFail = onFailOverride
    }
    if (onFailOverride !== 'download') {
      removeRuntimeDependencies(manifest[dependenciesFieldName], runtimes)
    } else {
      convertEnginesRuntimeToDependencies(manifest, enginesFieldName, dependenciesFieldName)
    }
  }
}

function removeRuntimeDependencies (deps: Record<string, string> | undefined, runtimes: EngineDependency[]): void {
  if (!deps) return
  for (const runtimeName of RUNTIME_NAMES) {
    if (
      runtimes.some(runtime => runtime.name === runtimeName) &&
      typeof deps[runtimeName] === 'string' &&
      deps[runtimeName].startsWith('runtime:')
    ) {
      delete deps[runtimeName]
    }
  }
}

function toRuntimeList (enginesFieldRuntime: EngineDependency | EngineDependency[] | undefined): EngineDependency[] | undefined {
  if (enginesFieldRuntime == null) return undefined
  return Array.isArray(enginesFieldRuntime) ? enginesFieldRuntime : [enginesFieldRuntime]
}
