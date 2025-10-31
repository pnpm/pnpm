import { globalWarn } from '@pnpm/logger'
import {
  type DependenciesField,
  type EngineDependency,
  type ProjectManifest,
} from '@pnpm/types'

export function convertEnginesRuntimeToDependencies (
  manifest: ProjectManifest,
  enginesFieldName: 'devEngines' | 'engines',
  dependenciesFieldName: DependenciesField
): void {
  for (const runtimeName of ['node', 'deno', 'bun']) {
    const enginesFieldRuntime = manifest[enginesFieldName]?.runtime
    if (enginesFieldRuntime == null || manifest[dependenciesFieldName]?.[runtimeName]) {
      continue
    }
    const runtimes: EngineDependency[] = Array.isArray(enginesFieldRuntime) ? enginesFieldRuntime : [enginesFieldRuntime]
    const runtime = runtimes.find((runtime) => runtime.name === runtimeName)
    if (runtime?.onFail !== 'download') {
      continue
    }
    if ('webcontainer' in process.versions) {
      globalWarn(`Installation of ${runtimeName} versions is not supported in WebContainer`)
    } else {
      manifest[dependenciesFieldName] ??= {}
      manifest[dependenciesFieldName]![runtimeName] = `runtime:${runtime.version}`
    }
  }
}
