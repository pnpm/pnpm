import type { DevEngines, EngineDependency, ProjectManifest } from '@pnpm/types'
import semver from 'semver'

type RuntimeOnFail = 'ignore' | 'warn' | 'error' | 'download'

interface DeclaredNodeRuntime {
  version: string | undefined
  downloaded: boolean
  specifier: string
}

/**
 * The Node.js version declared by `devEngines.runtime` or `engines.runtime`,
 * in that precedence order. A range names a version only when pnpm downloads
 * it, since a range it does not download stands for the system Node.js.
 */
export function getNodeVersionFromEnginesRuntime (
  manifest: ProjectManifest,
  dependencyGroup?: 'dependencies' | 'devDependencies'
): string | undefined {
  return findDeclaredNodeRuntime(manifest, undefined, dependencyGroup)?.version
}

/**
 * {@link getNodeVersionFromEnginesRuntime} when pnpm downloads that Node.js,
 * with `onFailOverride` (the `runtimeOnFail` setting) replacing the entry's
 * `onFail`. `undefined` when the project runs on the system Node.js.
 */
export function getDownloadedNodeVersionFromEnginesRuntime (
  manifest: ProjectManifest,
  onFailOverride?: RuntimeOnFail,
  dependencyGroup?: 'dependencies' | 'devDependencies'
): string | undefined {
  const enginesFields = dependencyGroup === 'dependencies'
    ? ['engines'] as const
    : dependencyGroup === 'devDependencies'
      ? ['devEngines'] as const
      : ['engines', 'devEngines'] as const
  for (const enginesFieldName of enginesFields) {
    const runtime = declaredNodeRuntime(manifest[enginesFieldName]?.runtime, onFailOverride)
    if (runtime?.downloaded) return runtime.version
  }
  return undefined
}

export function getDownloadedNodeRuntimeSpecifierFromEnginesRuntime (
  manifest: ProjectManifest,
  onFailOverride?: RuntimeOnFail,
  dependencyGroup?: 'dependencies' | 'devDependencies'
): string | undefined {
  const enginesFields = dependencyGroup === 'dependencies'
    ? ['engines'] as const
    : dependencyGroup === 'devDependencies'
      ? ['devEngines'] as const
      : ['engines', 'devEngines'] as const
  for (const enginesFieldName of enginesFields) {
    const runtime = declaredNodeRuntime(manifest[enginesFieldName]?.runtime, onFailOverride)
    if (runtime?.downloaded) return `runtime:${runtime.specifier}`
  }
  return undefined
}

function findDeclaredNodeRuntime (
  manifest: ProjectManifest,
  onFailOverride?: RuntimeOnFail,
  dependencyGroup?: 'dependencies' | 'devDependencies'
): DeclaredNodeRuntime | undefined {
  const enginesFields = dependencyGroup === 'dependencies'
    ? ['engines'] as const
    : dependencyGroup === 'devDependencies'
      ? ['devEngines'] as const
      : ['devEngines', 'engines'] as const
  for (const enginesFieldName of enginesFields) {
    const declared = declaredNodeRuntime(manifest[enginesFieldName]?.runtime, onFailOverride)
    if (declared != null) return declared
  }
  return undefined
}

/**
 * Returns undefined when this runtime field names no usable Node.js version,
 * so the next field is consulted. A defined result ends the search, even when
 * the version it carries is undefined.
 */
function declaredNodeRuntime (
  enginesRuntime: DevEngines['runtime'],
  onFailOverride: RuntimeOnFail | undefined
): DeclaredNodeRuntime | undefined {
  if (enginesRuntime == null) return undefined
  const runtimes: Array<EngineDependency | null> = Array.isArray(enginesRuntime) ? enginesRuntime : [enginesRuntime]
  const nodeRuntime = runtimes.find((runtime) => runtime?.name === 'node')
  if (typeof nodeRuntime?.version !== 'string') return undefined
  const version = nodeRuntime.version.trim()
  if (!semver.validRange(version)) return undefined
  if ((onFailOverride ?? nodeRuntime.onFail) !== 'download') {
    return { version: semver.valid(version) ?? undefined, downloaded: false, specifier: version }
  }
  const minVersion = semver.minVersion(version)
  return minVersion != null ? { version: minVersion.version, downloaded: true, specifier: version } : undefined
}
