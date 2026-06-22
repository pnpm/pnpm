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
    const enginesFieldRuntime = manifest[enginesFieldName]?.runtime
    if (enginesFieldRuntime == null || manifest[dependenciesFieldName]?.[runtimeName]) {
      continue
    }
    const runtimes: EngineDependency[] = Array.isArray(enginesFieldRuntime) ? enginesFieldRuntime : [enginesFieldRuntime]
    const runtime = runtimes.find((runtime) => runtime.name === runtimeName)
    if (runtime?.onFail !== 'download') {
      continue
    }
    if (typeof runtime.version !== 'string') {
      globalWarn(`Cannot download ${runtimeName} because no version is specified in ${enginesFieldName}.runtime`)
      continue
    }
    const version = runtime.version.trim()
    if ('webcontainer' in process.versions) {
      globalWarn(`Installation of ${runtimeName} versions is not supported in WebContainer`)
    } else {
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
  }
}

export function applyRuntimeOnFailOverride (
  manifest: ProjectManifest,
  onFailOverride: 'ignore' | 'warn' | 'error' | 'download'
): void {
  for (const [enginesFieldName, dependenciesFieldName] of [
    ['devEngines', 'devDependencies'],
    ['engines', 'dependencies'],
  ] as const) {
    const enginesFieldRuntime = manifest[enginesFieldName]?.runtime
    if (enginesFieldRuntime == null) continue
    const runtimes: EngineDependency[] = Array.isArray(enginesFieldRuntime) ? enginesFieldRuntime : [enginesFieldRuntime]
    for (const runtime of runtimes) {
      runtime.onFail = onFailOverride
    }
    if (onFailOverride !== 'download') {
      const deps = manifest[dependenciesFieldName]
      if (deps) {
        for (const runtimeName of RUNTIME_NAMES) {
          if (typeof deps[runtimeName] === 'string' && deps[runtimeName].startsWith('runtime:')) {
            delete deps[runtimeName]
          }
        }
      }
    } else {
      convertEnginesRuntimeToDependencies(manifest, enginesFieldName, dependenciesFieldName)
    }
  }
}
