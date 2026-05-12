import { globalWarn } from '@pnpm/logger'
import {
  type DependenciesField,
  type EngineDependency,
  isProtoPollutionKey,
  type ProjectManifest,
} from '@pnpm/types'

const RUNTIME_NAMES = ['node', 'deno', 'bun'] as const

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
    if (!runtime.version) {
      globalWarn(`Cannot download ${runtimeName} because no version is specified in ${enginesFieldName}.runtime`)
      continue
    }
    if ('webcontainer' in process.versions) {
      globalWarn(`Installation of ${runtimeName} versions is not supported in WebContainer`)
    } else {
      // The barrier is unreachable for the current `RUNTIME_NAMES`, but it keeps
      // the assignment safe (and CodeQL js/prototype-polluting-assignment quiet)
      // if a future entry is added to that list.
      if (isProtoPollutionKey(runtimeName)) continue
      const deps = (manifest[dependenciesFieldName] ??= {})
      deps[runtimeName] = `runtime:${runtime.version}`
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
