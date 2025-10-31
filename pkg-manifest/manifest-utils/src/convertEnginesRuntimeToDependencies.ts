import { globalWarn } from '@pnpm/logger'
import {
  type DependenciesField,
  type DevEngineDependency,
  type ProjectManifest,
} from '@pnpm/types'

export function convertEnginesRuntimeToDependencies (manifest: ProjectManifest, enginesFieldName: 'devEngines' | 'engines', dependenciesFieldName: DependenciesField): ProjectManifest {
  for (const runtimeName of ['node', 'deno', 'bun']) {
    if (manifest[enginesFieldName]?.runtime && !manifest[dependenciesFieldName]?.[runtimeName]) {
      const runtimes: DevEngineDependency[] = Array.isArray(manifest[enginesFieldName]!.runtime) ? manifest[enginesFieldName]!.runtime! : [manifest[enginesFieldName]!.runtime!]
      const runtime = runtimes.find((runtime) => runtime.name === runtimeName)
      if (runtime && runtime.onFail === 'download') {
        if ('webcontainer' in process.versions) {
          globalWarn(`Installation of ${runtimeName} versions is not supported in WebContainer`)
        } else {
          manifest[dependenciesFieldName] ??= {}
          manifest[dependenciesFieldName]![runtimeName] = `runtime:${runtime.version}`
        }
      }
    }
  }
  return manifest
}

