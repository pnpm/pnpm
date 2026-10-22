import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import { matchCatalogResolveResult, resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import { getWantedDependencies, type ManifestWantedDependency } from '@pnpm/installing.deps-resolver'
import { logger } from '@pnpm/logger'
import { parseNpmAliasTarget } from '@pnpm/resolving.npm-resolver'
import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'
import semver from 'semver'

import type { ProcessedInstallOptions as StrictInstallOptions } from './extendInstallOptions.js'
import { forgetResolutionsOfPrevWantedDeps } from './forgetResolutions.js'
import type { ImporterToUpdate } from './mutationTypes.js'
import { getHookOwnedAliases, type InstallCaseProject, pickCatalogSpecifier, type ProjectCollector } from './projectCollector.js'

export async function installCase (collector: ProjectCollector, project: InstallCaseProject): Promise<void> {
  const { ctx, opts } = collector.run
  const hookOwnedAliases = getHookOwnedAliases(collector, project)
  const wantedDependencies = getWantedDependencies(project.manifest, {
    autoInstallPeers: opts.autoInstallPeers,
    includeDirect: opts.includeDirect,
  })
    .map((wantedDependency) => hookOwnedAliases?.has(wantedDependency.alias)
      ? { ...wantedDependency, saveSpec: false, updateToLatestAllowed: false, updateSpec: true }
      : { ...wantedDependency, updateSpec: true })
  if (opts.packageVulnerabilityAudit) {
    widenVulnerablePinnedSpecifiers(opts, {
      audit: opts.packageVulnerabilityAudit,
      hookOwnedAliases,
      rootDir: project.rootDir,
      wantedDependencies,
    })
  }

  if (ctx.wantedLockfile?.importers) {
    forgetResolutionsOfPrevWantedDeps(wantedDependencies, {
      importer: ctx.wantedLockfile.importers[project.id],
      prevCatalogs: ctx.wantedLockfile.catalogs,
      catalogsConfig: opts.catalogs,
    })
  }
  if (opts.ignoreScripts && definesInstallScripts(project.manifest)) {
    ctx.pendingBuilds.push(project.id)
  }

  collector.projectsToInstall.push({
    pruneDirectDependencies: false,
    ...project,
    hookOwnedAliases,
    wantedDependencies,
  } as ImporterToUpdate)
}

function definesInstallScripts (manifest: ProjectManifest | undefined): boolean {
  const scripts = manifest?.scripts
  return Boolean(scripts && (
    scripts.preinstall != null ||
    scripts.install != null ||
    scripts.postinstall != null ||
    scripts.prepare
  ))
}

interface PinnedVersion {
  catalogName: string | null
  npmAliasTarget: ReturnType<typeof parseNpmAliasTarget>
  packageName: string
  specifier: string | undefined
  version: string
}

/**
 * If the current version is pinned and vulnerable, expand the specifier to a range
 * that will allow updating to a non-vulnerable, semver-compatible version, if available.
 */
function widenVulnerablePinnedSpecifiers (
  opts: StrictInstallOptions,
  { audit, hookOwnedAliases, rootDir, wantedDependencies }: {
    audit: NonNullable<StrictInstallOptions['packageVulnerabilityAudit']>
    hookOwnedAliases: Set<string> | undefined
    rootDir: ProjectRootDir
    wantedDependencies: ManifestWantedDependency[]
  }
): void {
  for (const dep of wantedDependencies) {
    const pinned = findPinnedVersion(opts.catalogs, dep)
    if (pinned == null) continue
    if (!audit.isVulnerable(pinned.packageName, pinned.version)) continue
    if (hookOwnedAliases?.has(dep.alias)) {
      // An update reaches a vulnerable version by widening the specifier the project
      // declares, and this one is not the project's to widen. An override outranks every
      // other hook, so that is the fix to point at.
      logger.warn({
        message: `Cannot update "${dep.alias}" away from ${pinned.version}: its specifier "${pinned.specifier!}" comes from a package extension, readPackage hook, or override, not from the project manifest. Run "pnpm audit --fix" to add an override for it instead.`,
        prefix: rootDir,
      })
      continue
    }
    widenPinnedSpecifier(opts, { dep, pinned })
  }
}

function findPinnedVersion (catalogs: Catalogs, dep: ManifestWantedDependency): PinnedVersion | undefined {
  let specifier: string | undefined = dep.bareSpecifier
  const catalogName = specifier ? parseCatalogProtocol(specifier) : null
  if (catalogName != null) {
    const catalogResult = resolveFromCatalog(catalogs, { alias: dep.alias, bareSpecifier: specifier! })
    specifier = matchCatalogResolveResult(catalogResult, pickCatalogSpecifier)
  }
  const npmAliasTarget = specifier != null ? parseNpmAliasTarget(specifier, dep.alias) : null
  const packageName = npmAliasTarget?.name ?? dep.alias
  let versionSelector = npmAliasTarget != null ? npmAliasTarget.versionSelector : specifier
  // `=1.0.0` pins as exactly as `1.0.0` does, but semver.valid() only accepts the bare version.
  if (versionSelector?.startsWith('=')) versionSelector = versionSelector.slice(1)
  const validVersion = semver.valid(versionSelector)
  if (!validVersion) return undefined
  return { catalogName, npmAliasTarget, packageName, specifier, version: validVersion }
}

function widenPinnedSpecifier (
  opts: StrictInstallOptions,
  { dep, pinned }: { dep: ManifestWantedDependency, pinned: PinnedVersion }
): void {
  const { catalogName, npmAliasTarget, version } = pinned
  // The widened specifier keeps the npm alias shape so that it still names the
  // real package rather than the alias.
  const widenedSpecifier = npmAliasTarget != null
    ? `npm:${npmAliasTarget.name}@^${version}`
    : `^${version}`
  if (catalogName != null && opts.catalogs?.[catalogName]) {
    // If a catalog is used, update the catalog entry so the resolver can find a
    // non-vulnerable version. The package.json keeps "catalog:" and the workspace manifest
    // gets updated.
    opts.catalogs = {
      ...opts.catalogs,
      [catalogName]: {
        ...opts.catalogs[catalogName],
        [dep.alias]: widenedSpecifier,
      },
    }
    // Set prevSpecifier to the original catalog specifier so the resolver
    // preserves the original pinning style (i.e. pinned stays pinned).
    dep.prevSpecifier = pinned.specifier
  } else {
    dep.bareSpecifier = widenedSpecifier
  }
}
