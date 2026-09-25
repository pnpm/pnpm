import { resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { DEPENDENCIES_FIELDS, type IncludedDependencies, type ProjectManifest } from '@pnpm/types'
import semver from 'semver'

export interface FindInjectedWorkspaceDepOptions {
  /** Every workspace project's manifest, which names the packages a spec can resolve to. */
  workspaceManifests: ProjectManifest[]
  injectWorkspacePackages?: boolean
  include?: IncludedDependencies
  catalogs?: Catalogs
}

/**
 * Returns the name of the first dependency the install would inject, or
 * `undefined` when there is none.
 *
 * An injected dependency is a hard-linked copy of a workspace project. Its
 * contents change when that project is rebuilt, with no manifest or lockfile
 * moving, so the checks that only track mtimes cannot tell whether the copy
 * is still current (https://github.com/pnpm/pnpm/issues/4407).
 *
 * A dependency is injected when `dependenciesMeta` says so, or when
 * `injectWorkspacePackages` is on and the dependency resolves to a workspace
 * project. Dependency groups the install leaves out (per `include`) are
 * skipped, since their dependencies are not installed.
 */
export function findInjectedWorkspaceDep (manifests: ProjectManifest[], opts: FindInjectedWorkspaceDepOptions): string | undefined {
  const workspacePackages = new Map<string, string | undefined>()
  for (const { name, version } of opts.workspaceManifests) {
    if (typeof name === 'string') workspacePackages.set(name, typeof version === 'string' ? version : undefined)
  }
  for (const manifest of manifests) {
    for (const depField of DEPENDENCIES_FIELDS) {
      if (opts.include?.[depField] === false) continue
      const deps = manifest[depField]
      if (deps == null) continue
      for (const [alias, spec] of Object.entries(deps)) {
        if (manifest.dependenciesMeta?.[alias]?.injected === true) return alias
        if (opts.injectWorkspacePackages !== true) continue
        // A malformed manifest may carry a non-string spec; skip it rather
        // than throw, as checkDepsStatus() must never crash.
        if (typeof spec !== 'string') continue
        const actualSpec = dereferenceCatalog(alias, spec, opts.catalogs)
        if (actualSpec != null && resolvesToWorkspacePackage(alias, actualSpec, workspacePackages)) return alias
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

function resolvesToWorkspacePackage (alias: string, spec: string, workspacePackages: Map<string, string | undefined>): boolean {
  if (spec.startsWith('workspace:')) {
    const workspaceSpec = spec.slice('workspace:'.length)
    if (isPath(workspaceSpec)) return true
    const { name, range } = splitNameAndRange(alias, workspaceSpec)
    return workspacePackages.has(name) && rangeMatches(range, workspacePackages.get(name))
  }
  const { name, range } = splitNameAndRange(alias, spec.startsWith('npm:') ? spec.slice('npm:'.length) : spec)
  return workspacePackages.has(name) && rangeMatches(range, workspacePackages.get(name))
}

function isPath (spec: string): boolean {
  return spec.startsWith('.') || spec.startsWith('/') || spec.startsWith('~')
}

/** `name@range`, or a bare range that belongs to `alias`. */
function splitNameAndRange (alias: string, spec: string): { name: string, range: string } {
  const at = spec.lastIndexOf('@')
  return at > 0 ? { name: spec.slice(0, at), range: spec.slice(at + 1) } : { name: alias, range: spec }
}

/**
 * Whether `version` may be what `range` picks. An empty or wildcard range
 * matches anything, and so does one this check cannot parse, because an
 * install that then runs anyway costs less than one that is wrongly skipped.
 */
function rangeMatches (range: string, version: string | undefined): boolean {
  if (range === '' || range === '*' || range === '^' || range === '~') return true
  if (version == null || semver.validRange(range) == null) return true
  return semver.satisfies(version, range, { includePrerelease: true })
}
