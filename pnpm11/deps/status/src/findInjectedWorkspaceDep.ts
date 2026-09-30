import { resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { DEPENDENCIES_FIELDS, type IncludedDependencies, type ProjectManifest } from '@pnpm/types'
import { once } from 'ramda'
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
  const ctx: InjectedDepSearchContext = {
    ...opts,
    getWorkspacePackages: once(() => indexWorkspacePackages(opts.workspaceManifests)),
    linkWorkspacePackages: opts.linkWorkspacePackages !== false,
  }
  for (const manifest of manifests) {
    const alias = findInjectedDepInManifest(manifest, ctx)
    if (alias != null) return alias
  }
  return undefined
}

type WorkspacePackageVersions = Map<string, Array<string | undefined>>

interface InjectedDepSearchContext extends Omit<FindInjectedWorkspaceDepOptions, 'linkWorkspacePackages'> {
  getWorkspacePackages: () => WorkspacePackageVersions
  linkWorkspacePackages: boolean
}

function indexWorkspacePackages (workspaceManifests: ProjectManifest[]): WorkspacePackageVersions {
  const workspacePackages: WorkspacePackageVersions = new Map()
  for (const { name, version } of workspaceManifests) {
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

function findInjectedDepInManifest (manifest: ProjectManifest, ctx: InjectedDepSearchContext): string | undefined {
  for (const depField of DEPENDENCIES_FIELDS) {
    if (ctx.include?.[depField] === false) continue
    for (const [alias, spec] of Object.entries(manifest[depField] ?? {})) {
      if (manifest.dependenciesMeta?.[alias]?.injected === true || resolvesToInjectedWorkspacePackage(alias, spec, ctx)) return alias
    }
  }
  return undefined
}

function resolvesToInjectedWorkspacePackage (alias: string, spec: unknown, ctx: InjectedDepSearchContext): boolean {
  if (ctx.injectWorkspacePackages !== true) return false
  if (typeof spec !== 'string') return false
  const actualSpec = dereferenceCatalog(alias, spec, ctx.catalogs)
  return actualSpec != null && resolvesToWorkspacePackage(alias, actualSpec, ctx.getWorkspacePackages(), ctx.linkWorkspacePackages)
}

function dereferenceCatalog (alias: string, spec: string, catalogs?: Catalogs): string | undefined {
  if (!spec.startsWith('catalog:')) return spec
  const result = resolveFromCatalog(catalogs ?? {}, { alias, bareSpecifier: spec })
  return result.type === 'found' ? result.resolution.specifier : undefined
}

function resolvesToWorkspacePackage (
  alias: string,
  spec: string,
  workspacePackages: WorkspacePackageVersions,
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
