import { resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { DEPENDENCIES_FIELDS, type IncludedDependencies, type ProjectManifest } from '@pnpm/types'
import semver from 'semver'

export interface FindInjectedWorkspaceDepOptions {
  /** Every workspace project's manifest, which names the packages a spec can resolve to. */
  workspaceManifests: ProjectManifest[]
  injectWorkspacePackages?: boolean
  linkWorkspacePackages?: boolean | 'deep'
  include?: IncludedDependencies
  catalogs?: Catalogs
}

/**
 * Returns the name of the first dependency the install would inject, or
 * `undefined` when there is none.
 */
export function findInjectedWorkspaceDep (manifests: ProjectManifest[], opts: FindInjectedWorkspaceDepOptions): string | undefined {
  let workspacePackages: Map<string, Array<string | undefined>> | undefined
  const getWorkspacePackages = (): Map<string, Array<string | undefined>> => {
    if (workspacePackages != null) return workspacePackages
    workspacePackages = new Map()
    for (const { name, version } of opts.workspaceManifests) {
      if (typeof name !== 'string') continue
      const ver = typeof version === 'string' ? version : undefined
      const versions = workspacePackages.get(name)
      if (versions != null) {
        versions.push(ver)
      } else {
        workspacePackages.set(name, [ver])
      }
    }
    return workspacePackages
  }
  const linkWorkspacePackages = opts.linkWorkspacePackages !== false
  for (const manifest of manifests) {
    for (const depField of DEPENDENCIES_FIELDS) {
      if (opts.include?.[depField] === false) continue
      const deps = manifest[depField]
      if (deps == null) continue
      for (const [alias, spec] of Object.entries(deps)) {
        if (manifest.dependenciesMeta?.[alias]?.injected === true) return alias
        if (opts.injectWorkspacePackages !== true) continue
        if (typeof spec !== 'string') continue
        const actualSpec = dereferenceCatalog(alias, spec, opts.catalogs)
        if (actualSpec != null && resolvesToWorkspacePackage(alias, actualSpec, getWorkspacePackages(), linkWorkspacePackages)) return alias
      }
    }
  }
  return undefined
}

function dereferenceCatalog (alias: string, spec: string, catalogs?: Catalogs): string | undefined {
  if (!spec.startsWith('catalog:')) return spec
  const result = resolveFromCatalog(catalogs ?? {}, { alias, bareSpecifier: spec })
  return result.type === 'found' ? result.resolution.specifier : undefined
}

function resolvesToWorkspacePackage (
  alias: string,
  spec: string,
  workspacePackages: Map<string, Array<string | undefined>>,
  linkWorkspacePackages: boolean
): boolean {
  if (spec.startsWith('workspace:')) {
    const workspaceSpec = spec.slice('workspace:'.length)
    if (isPath(workspaceSpec)) return true
    const { name, range } = splitNameAndRange(alias, workspaceSpec)
    const versions = workspacePackages.get(name)
    return versions != null && versions.some((version) => rangeMatches(range, version, true))
  }
  if (!linkWorkspacePackages) return false
  const { name, range } = splitNameAndRange(alias, spec)
  const versions = workspacePackages.get(name)
  return versions != null && versions.some((version) => rangeMatches(range, version, false))
}

function isPath (spec: string): boolean {
  return spec.startsWith('.') || spec.startsWith('/') || spec.startsWith('~')
}

/** `name@range`, or a bare range that belongs to `alias`. */
function splitNameAndRange (alias: string, spec: string): { name: string, range: string } {
  if (spec.startsWith('npm:')) {
    const body = spec.slice('npm:'.length)
    if (semver.validRange(body) != null) {
      return { name: alias, range: body }
    }
    const at = body.lastIndexOf('@')
    if (at < 1) return { name: body, range: '*' }
    return { name: body.slice(0, at), range: body.slice(at + 1) }
  }
  const at = spec.lastIndexOf('@')
  return at > 0 ? { name: spec.slice(0, at), range: spec.slice(at + 1) } : { name: alias, range: spec }
}

function rangeMatches (range: string, version: string | undefined, isWorkspaceProtocol: boolean): boolean {
  if (range === '' || range === '*' || range === '^' || range === '~') return true
  if (semver.validRange(range) == null) return isWorkspaceProtocol
  if (version == null) return true
  return semver.satisfies(version, range, { includePrerelease: true })
}
