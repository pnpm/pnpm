import { resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { parseOverrides, type VersionOverride } from '@pnpm/config.parse-overrides'
import { createOverriddenDependencyMatcher, type OverriddenDependencyMatcher } from '@pnpm/hooks.read-package-hook'
import { DEPENDENCIES_FIELDS, type IncludedDependencies, type ProjectManifest } from '@pnpm/types'
import { isEmpty } from 'ramda'

import { findInjectedWorkspaceDep } from './findInjectedWorkspaceDep.js'
import type { CheckDepsStatusOptions } from './types.js'

/**
 * Describes the first dependency whose installed contents may have changed
 * without any manifest or lockfile mtime moving: a local file dependency, an
 * injected workspace dependency, or an override or package extension that
 * maps to a local file dependency. `undefined` when there is none.
 */
export function findLocalFileDepsIssue (opts: CheckDepsStatusOptions): string | undefined {
  // `parseOverrides` throws on a misconfigured catalog or invalid selector.
  // The outer catch in `checkDepsStatus` then reports the status as unknown,
  // and the resulting full install surfaces the same error.
  const overrides = opts.overrides != null && !isEmpty(opts.overrides)
    ? parseOverrides(opts.overrides, opts.catalogs)
    : []
  const manifests = listManifestsToScan(opts)
  const localFileDepContext: LocalFileDepSearchContext = {
    include: opts.include,
    catalogs: opts.catalogs,
    // An empty manifest is the parent of no `parent>dep` override, so only
    // overrides without a parent selector apply. Whether a parent-scoped one
    // applies depends on which package declares the dependency.
    isOverridden: createOverriddenDependencyMatcher(overrides, opts.workspaceDir ?? opts.rootProjectManifestDir)?.({}),
  }
  const localFileDep = findLocalFileDep(manifests, localFileDepContext)
  if (localFileDep != null) {
    return `The dependency "${localFileDep}" is a local file dependency and its contents may have changed`
  }
  const injectedWorkspaceDep = findInjectedWorkspaceDep(manifests, {
    workspaceManifests: manifests,
    injectWorkspacePackages: opts.injectWorkspacePackages,
    linkWorkspacePackages: opts.linkWorkspacePackages,
    include: opts.include,
    catalogs: opts.catalogs,
  })
  if (injectedWorkspaceDep != null) {
    return `The dependency "${injectedWorkspaceDep}" is an injected workspace dependency and its contents may have changed`
  }
  const localFileOverride = findLocalFileOverride(overrides)
  if (localFileOverride != null) {
    return `The override "${localFileOverride}" maps to a local file dependency and its contents may have changed`
  }
  const localFileExtension = findLocalFilePackageExtension(opts.packageExtensions, localFileDepContext)
  if (localFileExtension != null) {
    return `The package extension "${localFileExtension}" injects a local file dependency and its contents may have changed`
  }
  return undefined
}

function listManifestsToScan ({ allProjects, rootProjectManifest, rootProjectManifestDir }: CheckDepsStatusOptions): ProjectManifest[] {
  const manifests = allProjects?.map(({ manifest }) => manifest) ?? []
  // `rootProjectManifest` is tracked separately from `allProjects` and the
  // recursive project list can omit the workspace root (for example when
  // `includeWorkspaceRoot` is false), so scan it too unless `allProjects`
  // already covers it.
  if (rootProjectManifest != null && !allProjects?.some(({ rootDir }) => rootDir === rootProjectManifestDir)) {
    manifests.push(rootProjectManifest)
  }
  return manifests
}

interface LocalFileDepSearchContext {
  include?: IncludedDependencies
  catalogs?: Catalogs
  isOverridden?: OverriddenDependencyMatcher
}

/**
 * Returns the name of the first dependency declared with a local file
 * specifier in any of the given manifests, or `undefined` when there is none.
 * `link:` dependencies are excluded: they are symlinked, so changes inside
 * them flow through without a reinstall. Dependency groups excluded from the
 * current install (per `include`) are skipped: their local file dependencies
 * are not installed, so their contents cannot be stale. `catalog:` specs are
 * dereferenced through the catalogs config: the catalog resolver only bans
 * the `link:` and `file:` protocols, so a catalog entry can
 * still hold a bare local path (`../lib`, `vendor/pkg.tgz`) that resolves to
 * a local file dependency. Dependencies an override replaces are skipped:
 * the override's target is installed instead.
 */
function findLocalFileDep (manifests: ProjectManifest[], ctx: LocalFileDepSearchContext): string | undefined {
  for (const manifest of manifests) {
    for (const depField of DEPENDENCIES_FIELDS) {
      if (ctx.include?.[depField] === false) continue
      const depName = findLocalFileDepInRecord(manifest[depField], ctx)
      if (depName != null) return depName
    }
  }
  return undefined
}

/**
 * Returns the name of the first dependency in `deps` declared with (or
 * resolving through a catalog to) a local file specifier and not replaced by
 * an override, or `undefined`.
 */
function findLocalFileDepInRecord (deps: Record<string, string> | undefined, { catalogs, isOverridden }: LocalFileDepSearchContext): string | undefined {
  if (deps == null) return undefined
  for (const [depName, spec] of Object.entries(deps)) {
    // A malformed manifest may carry a non-string spec; skip it rather
    // than throw — checkDepsStatus() must never crash.
    if (typeof spec !== 'string') continue
    if (!isEffectiveLocalFileSpec(depName, spec, catalogs)) continue
    if (isOverridden?.(depName, spec)) continue
    return depName
  }
  return undefined
}

/**
 * Whether the dependency's specifier is (or resolves through a catalog to) a
 * local file specifier.
 */
function isEffectiveLocalFileSpec (depName: string, spec: string, catalogs?: Catalogs): boolean {
  if (isLocalFileSpec(spec)) return true
  // Only catalog: specs consult the catalogs, so skip the lookup for
  // everything else to keep the optimistic fast path cheap.
  if (!spec.startsWith('catalog:')) return false
  const catalogResult = resolveFromCatalog(catalogs ?? {}, { alias: depName, bareSpecifier: spec })
  return catalogResult.type === 'found' && isLocalFileSpec(catalogResult.resolution.specifier)
}

/**
 * Returns the selector of the first `packageExtensions` entry that injects a
 * local file dependency, or `undefined` when there is none. Package
 * extensions are merged into matching packages' manifests by a read-package
 * hook during install, so a `file:`/local-path/tarball spec added there has
 * the same content-change blind spot as a direct local file dependency
 * without appearing in any project manifest. Only `dependencies` and
 * `optionalDependencies` are scanned: peer dependencies are resolved from the
 * graph rather than fetched, so a local spec there is never installed.
 */
function findLocalFilePackageExtension (packageExtensions: CheckDepsStatusOptions['packageExtensions'], ctx: LocalFileDepSearchContext): string | undefined {
  if (packageExtensions == null) return undefined
  for (const [selector, extension] of Object.entries(packageExtensions)) {
    if (findLocalFileDepInRecord(extension.dependencies, ctx) != null) return selector
    if (ctx.include?.optionalDependencies === false) continue
    if (findLocalFileDepInRecord(extension.optionalDependencies, ctx) != null) return selector
  }
  return undefined
}

/**
 * Returns the selector of the first override that maps to a local file
 * specifier, or `undefined` when there is none. An override redirects every
 * matching dependency in the graph to its specifier, so a local file override
 * makes the installed contents depend on that directory or tarball the same
 * way a direct local file dependency does.
 */
function findLocalFileOverride (overrides: VersionOverride[]): string | undefined {
  return overrides.find(({ newBareSpecifier }) => isLocalFileSpec(newBareSpecifier))?.selector
}

const LOCAL_PATH_PREFIX = /^(?:[./\\]|~[/\\]|[a-z]:)/i
const LOCAL_TARBALL_EXTENSION = /\.(?:tgz|tar\.gz|tar|tar\.bz2|tbz2|tbz)$/i

/**
 * Whether the specifier resolves to a local directory or tarball whose
 * contents can change without any manifest or lockfile mtime moving: the
 * `file:` protocol, path-prefixed specs (`./`, `../`, `~/`, absolute POSIX
 * paths, and Windows drive paths — including drive-relative ones like
 * `C:dir`, matching the local resolver's `isFilespec`), and bare tarball
 * file names.
 *
 * Deliberately narrower than the local resolver's bare-path matching: a bare
 * `dir/file.tgz`-less path like `user/repo` is statically indistinguishable
 * from a git shorthand at this layer, and matching it would disable the
 * repeat-install fast path for every project with git dependencies. Such
 * specs (and anything else carrying a protocol or URL) stay on the fast
 * path. `catalog:` specs also return false here — callers dereference them
 * through the catalogs config first.
 */
function isLocalFileSpec (spec: string): boolean {
  if (spec.startsWith('file:')) return true
  if (LOCAL_PATH_PREFIX.test(spec)) return true
  if (spec.includes(':')) return false
  // A `#` here means a hosted-git shorthand committish (`user/repo#release.tgz`),
  // not a local tarball — the `file:` and path-prefixed cases already returned above.
  if (spec.includes('#')) return false
  return LOCAL_TARBALL_EXTENSION.test(spec)
}
