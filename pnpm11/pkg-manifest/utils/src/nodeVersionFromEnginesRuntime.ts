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
  return findDownloadedNodeRuntime(manifest, onFailOverride, dependencyGroup)?.version
}

export function getDownloadedNodeRuntimeSpecifierFromEnginesRuntime (
  manifest: ProjectManifest,
  onFailOverride?: RuntimeOnFail,
  dependencyGroup?: 'dependencies' | 'devDependencies'
): string | undefined {
  return findDownloadedNodeRuntime(manifest, onFailOverride, dependencyGroup)?.specifier
}

interface DownloadedNodeRuntime {
  specifier: string
  version: string
}

function findDownloadedNodeRuntime (
  manifest: ProjectManifest,
  onFailOverride?: RuntimeOnFail,
  dependencyGroup?: 'dependencies' | 'devDependencies'
): DownloadedNodeRuntime | undefined {
  return findDownloadedNodeRuntimeFromDependencies(manifest, onFailOverride, dependencyGroup) ??
    findDownloadedNodeRuntimeFromEngines(manifest, onFailOverride, dependencyGroup)
}

function findDownloadedNodeRuntimeFromDependencies (
  manifest: ProjectManifest,
  onFailOverride?: RuntimeOnFail,
  dependencyGroup?: 'dependencies' | 'devDependencies'
): DownloadedNodeRuntime | undefined {
  const dependencyFields = dependencyGroup === 'dependencies'
    ? ['dependencies'] as const
    : dependencyGroup === 'devDependencies'
      ? ['devDependencies'] as const
      : ['dependencies', 'devDependencies'] as const
  for (const dependencyField of dependencyFields) {
    const specifier = manifest[dependencyField]?.node
    if (typeof specifier !== 'string') continue
    if (isRuntimeDependencyDisabledByOverride(manifest, dependencyField, specifier, onFailOverride)) continue
    const version = downloadedRuntimeVersion(specifier)
    if (version != null) return { specifier, version }
  }
  return undefined
}

function isRuntimeDependencyDisabledByOverride (
  manifest: ProjectManifest,
  dependencyField: 'dependencies' | 'devDependencies',
  specifier: string,
  onFailOverride: RuntimeOnFail | undefined
): boolean {
  if (onFailOverride == null || onFailOverride === 'download') return false
  const enginesField = dependencyField === 'dependencies' ? 'engines' : 'devEngines'
  const runtime = declaredNodeRuntime(manifest[enginesField]?.runtime, undefined)
  return specifier === `runtime:${runtime?.specifier}`
}

function findDownloadedNodeRuntimeFromEngines (
  manifest: ProjectManifest,
  onFailOverride?: RuntimeOnFail,
  dependencyGroup?: 'dependencies' | 'devDependencies'
): DownloadedNodeRuntime | undefined {
  const enginesFields = dependencyGroup === 'dependencies'
    ? ['engines'] as const
    : dependencyGroup === 'devDependencies'
      ? ['devEngines'] as const
      : ['engines', 'devEngines'] as const
  for (const enginesFieldName of enginesFields) {
    const runtime = declaredNodeRuntime(manifest[enginesFieldName]?.runtime, onFailOverride)
    if (runtime?.downloaded && runtime.version != null) {
      return { specifier: `runtime:${runtime.specifier}`, version: runtime.version }
    }
  }
  return undefined
}

function downloadedRuntimeVersion (specifier: string): string | undefined {
  const version = specifier.startsWith('runtime:')
    ? specifier.slice('runtime:'.length)
    : specifier.startsWith('tool:')
      ? specifier.slice('tool:'.length)
      : undefined
  return version == null ? undefined : semver.minVersion(version)?.version
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
