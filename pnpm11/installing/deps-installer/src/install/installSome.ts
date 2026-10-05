import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import { matchCatalogResolveResult, resolveFromCatalog } from '@pnpm/catalogs.resolver'
import { isWorkspaceLocalPathSpecifier, type WantedDependency } from '@pnpm/installing.deps-resolver'
import { logger } from '@pnpm/logger'
import { getAllDependenciesFromManifest } from '@pnpm/pkg-manifest.utils'
import { isLocalFilesystemSpecifier } from '@pnpm/resolving.local-resolver'
import { EXISTING_VERSION_SELECTOR_WEIGHT, type PreferredVersions } from '@pnpm/resolving.resolver-base'
import type { Dependencies } from '@pnpm/types'
import semver from 'semver'

import { isSameSource } from '../isSameSource.js'
import { type ParsedWantedDependencies, parseWantedDependencies } from '../parseWantedDependencies.js'
import { CatalogVersionMismatchError } from './checkCompatibility/CatalogVersionMismatchError.js'
import { getHookGovernedAdds } from './hookGovernedAdds.js'
import type { ImporterToUpdate } from './mutationTypes.js'
import { getHookOwnedAliases, type InstallSomeProject, pickCatalogSpecifier, type ProjectCollector } from './projectCollector.js'

interface DeclaredSpecifiers {
  currentBareSpecifiers: Dependencies
  originalBareSpecifiers: Dependencies
  readonlyAliases: Set<string> | undefined
  hookRemovedAliases: Set<string> | undefined
  hookSupersededSpecifiers: Map<string, string> | undefined
  hookGovernedAliases: Set<string> | undefined
  readonlySpecifiers: Dependencies | undefined
}

export async function installSome (collector: ProjectCollector, project: InstallSomeProject): Promise<void> {
  const { opts } = collector.run
  // The manifest keeps its specifiers, so they stay authoritative: whatever resolution settles
  // on has to satisfy them, or the lockfile importer entry contradicts itself and the next
  // frozen install rejects it.
  const readonlyManifest = project.update === true && !project.updatePackageManifest
  const declared = await readDeclaredSpecifiers(collector, project)
  const parsed = parseSelectors(collector, { declared, project, readonlyManifest })

  warnAboutDroppedSelectors(project, parsed)
  warnAboutKeptRanges(project, { declared, parsed })
  // `--latest` reaches past the declared range by design, which a manifest that keeps its
  // specifiers can't record. Degrade to an in-range update rather than write an entry the
  // next frozen install would reject.
  const updateToLatest = project.updateToLatest === true && !readonlyManifest
  if (project.updateToLatest === true && readonlyManifest) {
    logger.warn({
      message: 'Ignoring "--latest": the manifest keeps its version ranges when updating without saving, so dependencies were updated within them instead.',
      prefix: project.rootDir,
    })
  }

  if (opts.catalogMode !== 'manual') {
    promoteToCatalogs(collector, {
      catalogMode: opts.catalogMode,
      hookGovernedAliases: declared.hookGovernedAliases,
      wantedDeps: parsed.wantedDependencies,
    })
  }

  collector.projectsToInstall.push({
    pruneDirectDependencies: false,
    ...project,
    hookOwnedAliases: declared.readonlyAliases,
    updateToLatest,
    wantedDependencies: parsed.wantedDependencies.map((wantedDep) => markWantedDependency(project, { declared, wantedDep })),
  } as ImporterToUpdate)
}

async function readDeclaredSpecifiers (collector: ProjectCollector, project: InstallSomeProject): Promise<DeclaredSpecifiers> {
  const { opts } = collector.run
  const effectiveBareSpecifiers = getAllDependenciesFromManifest(project.manifest, {
    autoInstallPeers: opts.autoInstallPeers,
    peerAliases: project.peerAliases,
  })
  const currentBareSpecifiers = opts.ignoreCurrentSpecifiers
    ? {}
    : effectiveBareSpecifiers
  const originalBareSpecifiers = project.originalManifest == null
    ? currentBareSpecifiers
    : getAllDependenciesFromManifest(project.originalManifest, { autoInstallPeers: opts.autoInstallPeers, peerAliases: project.peerAliases })
  const readonlyAliases = getHookOwnedAliases(collector, project)
  const hookGovernedAdds = project.update === true ? undefined : await getHookGovernedAdds(collector, project)
  const hookSupersededSpecifiers = hookGovernedAdds?.superseded
  const hookGovernedAliases = combineHookGovernedAliases(readonlyAliases, hookSupersededSpecifiers)
  const readonlySpecifiers = hookGovernedAliases == null
    ? undefined
    : Object.fromEntries(
      Array.from(hookGovernedAliases, (alias) => [alias, hookSupersededSpecifiers?.get(alias) ?? effectiveBareSpecifiers[alias]])
    ) as Dependencies
  return {
    currentBareSpecifiers,
    originalBareSpecifiers,
    readonlyAliases,
    hookRemovedAliases: hookGovernedAdds?.removed,
    hookSupersededSpecifiers,
    hookGovernedAliases,
    readonlySpecifiers,
  }
}

function combineHookGovernedAliases (
  readonlyAliases: Set<string> | undefined,
  hookSupersededSpecifiers: Map<string, string> | undefined
): Set<string> | undefined {
  if (readonlyAliases != null) return new Set([...readonlyAliases, ...hookSupersededSpecifiers?.keys() ?? []])
  return hookSupersededSpecifiers == null ? undefined : new Set(hookSupersededSpecifiers.keys())
}

function parseSelectors (
  collector: ProjectCollector,
  { declared, project, readonlyManifest }: { declared: DeclaredSpecifiers, project: InstallSomeProject, readonlyManifest: boolean }
): ParsedWantedDependencies {
  const { opts } = collector.run
  const optionalDependencies = project.targetDependenciesField ? {} : project.manifest.optionalDependencies ?? {}
  const devDependencies = project.targetDependenciesField ? {} : project.manifest.devDependencies ?? {}
  return parseWantedDependencies(project.dependencySelectors, {
    allowNew: project.allowNew !== false,
    currentBareSpecifiers: declared.currentBareSpecifiers,
    defaultTag: opts.tag,
    dev: project.targetDependenciesField === 'devDependencies',
    devDependencies,
    optional: project.targetDependenciesField === 'optionalDependencies',
    optionalDependencies,
    updateWorkspaceDependencies: project.update,
    preferredSpecs: collector.getPreferredSpecs(),
    saveCatalogName: opts.saveCatalogName,
    overrides: opts.overrides,
    defaultCatalog: opts.catalogs?.default,
    readonlySpecifiers: declared.readonlySpecifiers,
    readonlyManifest,
    hookRemovedAliases: declared.hookRemovedAliases,
  })
}

function warnAboutDroppedSelectors (project: InstallSomeProject, parsed: ParsedWantedDependencies): void {
  for (const alias of parsed.removedByHook) {
    logger.warn({
      message: `Skipping "${alias}": a package extension, readPackage hook, or override removes it from the manifest, so it cannot be declared.`,
      prefix: project.rootDir,
    })
  }
  for (const wantedDep of parsed.wantedDependencies) {
    if (wantedDep.alias != null && wantedDep.prevSpecifier && !isSameSource(wantedDep.prevSpecifier, wantedDep.bareSpecifier, wantedDep.alias)) {
      logger.warn({
        message: `Replaced "${wantedDep.alias}" ("${wantedDep.prevSpecifier}") with "${wantedDep.bareSpecifier}" from a different source.`,
        prefix: project.rootDir,
      })
    }
  }
}

function warnAboutKeptRanges (
  project: InstallSomeProject,
  { declared, parsed }: { declared: DeclaredSpecifiers, parsed: ParsedWantedDependencies }
): void {
  const { readonlySpecifiers } = declared
  for (const { alias, requested, kept } of parsed.outsideKeptRange) {
    logger.warn({
      message: `Skipping "${alias}@${requested}": it doesn't satisfy "${kept}", which the manifest keeps when updating without saving.`,
      prefix: project.rootDir,
    })
  }
  for (const { alias, requested, kept } of parsed.supersededByKeptRange) {
    const message = readonlySpecifiers != null && Object.hasOwn(readonlySpecifiers, alias)
      ? `Ignoring "${alias}@${requested}": "${alias}" is controlled by a package extension, readPackage hook, or override, so its specifier "${kept}" was used instead.`
      : `Ignoring "${alias}@${requested}": the manifest keeps "${kept}" when updating without saving, so "${alias}" was updated within that range instead.`
    logger.warn({
      message,
      prefix: project.rootDir,
    })
  }
}

function markWantedDependency (
  project: InstallSomeProject,
  { declared, wantedDep }: { declared: DeclaredSpecifiers, wantedDep: WantedDependency }
): WantedDependency {
  const { hookGovernedAliases, hookSupersededSpecifiers, originalBareSpecifiers, readonlyAliases } = declared
  return {
    ...wantedDep,
    isNew: project.update !== true && (wantedDep.alias == null || !Object.hasOwn(originalBareSpecifiers, wantedDep.alias)),
    // A catalog name is enough to put the dependency in the lockfile's catalogs: the
    // resolver attaches a `catalogLookup` for it, and the entry that snapshot needs is not
    // this run's to write, so the next frozen install would reject the pair. `catalogMode:
    // manual` reaches here without passing the loop above, so the name is dropped here.
    saveCatalogName: wantedDep.alias != null && hookGovernedAliases?.has(wantedDep.alias) ? undefined : wantedDep.saveCatalogName,
    // A superseded request is no longer the manifest's new specifier, so it loses the
    // exemption `getDeclaredSpecifierOwnedByHook` grants an explicit one: a range or a
    // `catalog:` reference the project already declares stays as it is.
    saveSpec: wantedDep.alias != null && hookSupersededSpecifiers?.has(wantedDep.alias) ? undefined : wantedDep.alias == null || !readonlyAliases?.has(wantedDep.alias),
    updateToLatestAllowed: wantedDep.alias == null || !hookGovernedAliases?.has(wantedDep.alias),
    updateSpec: true,
  }
}

type CatalogPromotionMode = Exclude<ProjectCollector['run']['opts']['catalogMode'], 'manual'>

function promoteToCatalogs (
  collector: ProjectCollector,
  { catalogMode, hookGovernedAliases, wantedDeps }: {
    catalogMode: CatalogPromotionMode
    hookGovernedAliases: Set<string> | undefined
    wantedDeps: WantedDependency[]
  }
): void {
  for (const wantedDep of wantedDeps) {
    // Promotion moves the dependency onto the catalog entry's range, and the entry resolves
    // on its own from then on. A hook or an override supplies this specifier, so it is not
    // this run's to hand over.
    if (wantedDep.alias != null && hookGovernedAliases?.has(wantedDep.alias)) continue
    if (!canPromoteToCatalog(wantedDep)) continue
    promoteToCatalog(collector, { catalogMode, wantedDep })
  }
}

function canPromoteToCatalog (wantedDep: WantedDependency): boolean {
  // A `runtime:` specifier (e.g. node from `devEngines.runtime` or
  // `pnpm runtime set`) round-trips to `devEngines.runtime` through the
  // manifest writer, which only recognizes the `runtime:` protocol.
  // Promoting it into a catalog rewrites the entry to `catalog:`, which
  // breaks that round-trip and strands it in `devDependencies`.
  if (wantedDep.bareSpecifier?.startsWith('runtime:')) return false
  if (wantedDep.bareSpecifier != null && isProjectRelativePath(wantedDep.bareSpecifier)) return false
  return !(
    wantedDep.prevSpecifier != null &&
    parseCatalogProtocol(wantedDep.prevSpecifier) != null &&
    wantedDep.bareSpecifier !== wantedDep.prevSpecifier &&
    isExplicitDistTagSpecifier(wantedDep.bareSpecifier)
  )
}

function promoteToCatalog (
  collector: ProjectCollector,
  { catalogMode, wantedDep }: { catalogMode: CatalogPromotionMode, wantedDep: WantedDependency }
): void {
  const { opts } = collector.run
  const perDepCatalogName = getPerDepCatalogName(wantedDep, opts.saveCatalogName)
  const catalogBareSpecifier = `catalog:${perDepCatalogName === 'default' ? '' : perDepCatalogName}`
  const catalog = resolveFromCatalog(opts.catalogs, { ...wantedDep, bareSpecifier: catalogBareSpecifier })
  const catalogDepSpecifier = matchCatalogResolveResult(catalog, pickCatalogSpecifier)

  if (!catalogDepSpecifier || wantedDep.bareSpecifier === catalogBareSpecifier) {
    wantedDep.saveCatalogName = perDepCatalogName
    return
  }

  if (wantedDep.alias != null && catalogCovers(catalogDepSpecifier, wantedDep.bareSpecifier)) {
    // The catalog covers the wanted version, so the dependency resolves through the
    // catalog: every project referencing the entry stays on the one version the entry
    // resolves to. Keeping the wanted version as the specifier would pin it in the
    // manifest instead, dropping the project out of the catalog.
    resolveCatalogEntryTo(collector, {
      alias: wantedDep.alias,
      catalogName: perDepCatalogName,
      version: wantedDep.bareSpecifier!,
    })
    wantedDep.bareSpecifier = catalogBareSpecifier
    wantedDep.saveCatalogName = perDepCatalogName
    return
  }

  switch (catalogMode) {
    case 'strict':
      throw new CatalogVersionMismatchError({ catalogDep: `${wantedDep.alias}@${catalogDepSpecifier}`, wantedDep: `${wantedDep.alias}@${wantedDep.bareSpecifier}` })

    case 'prefer':
      logger.warn({
        message: `Catalog version mismatch for "${wantedDep.alias}": using direct version "${wantedDep.bareSpecifier}" instead of catalog version "${catalogDepSpecifier}".`,
        prefix: opts.lockfileDir,
      })
  }
}

/**
 * Resolve a catalog entry to the version a command asked for, leaving the range the
 * workspace declared for it alone.
 *
 * A cataloged dependency takes its version from the catalog, so the entry's recorded
 * resolution is what a wanted version has to move. Reusing it instead drops the request
 * without a word. Only the projects in this install follow the entry to the new version;
 * the rest keep their resolutions until they are installed, as they do for any other
 * catalog change.
 */
function resolveCatalogEntryTo (
  collector: ProjectCollector,
  { alias, catalogName, version }: {
    alias: string
    catalogName: string
    version: string
  }
): void {
  const { ctx, opts } = collector.run
  if (ctx.wantedLockfile.catalogs?.[catalogName]?.[alias]?.version === version) return

  delete ctx.wantedLockfile.catalogs?.[catalogName]?.[alias]
  forgetCatalogResolutionsOfInstalledProjects(collector, { alias, catalogName })

  // Outranks the pin the lockfile seeds for the package: naming a version is a request to
  // move off whatever is resolved now, and without that the pin wins.
  // Null-prototype merge targets so a package named `__proto__` lands as a plain own key
  // instead of invoking the prototype setter.
  const preferredVersions: PreferredVersions = Object.assign(Object.create(null), opts.preferredVersions)
  preferredVersions[alias] = Object.assign(Object.create(null), preferredVersions[alias], {
    [version]: { selectorType: 'version', weight: EXISTING_VERSION_SELECTOR_WEIGHT + 1 },
  })
  opts.preferredVersions = preferredVersions
}

function forgetCatalogResolutionsOfInstalledProjects (
  collector: ProjectCollector,
  { alias, catalogName }: { alias: string, catalogName: string }
): void {
  for (const [id, importer] of Object.entries(collector.run.ctx.wantedLockfile.importers ?? {})) {
    if (!collector.installedProjectIds.has(id)) continue
    const specifier = importer.specifiers?.[alias]
    if (specifier == null || parseCatalogProtocol(specifier) !== catalogName) continue
    delete importer.dependencies?.[alias]
    delete importer.devDependencies?.[alias]
    delete importer.optionalDependencies?.[alias]
  }
}

/**
 * Whether `specifier` names a path resolved against the project that declares
 * it — a `file:` / `link:` protocol, a bare path or tarball filename, or a
 * `workspace:` pointing at a directory rather than a range.
 *
 * A catalog entry is read by every project that references it, so it cannot
 * mean the same directory for all of them. `resolveFromCatalog` already
 * refuses a `link:` / `file:` entry outright
 * (`ERR_PNPM_CATALOG_ENTRY_INVALID_SPEC`); it accepts a `workspace:` one,
 * which is worse — every consumer silently resolves the relative path from its
 * own directory. Auto-cataloging leaves all of them alone.
 */
function isProjectRelativePath (specifier: string): boolean {
  return isLocalFilesystemSpecifier(specifier) || isWorkspaceLocalPathSpecifier(specifier)
}

/**
 * Whether the catalog entry already covers the wanted specifier, so the dependency can keep
 * resolving through the catalog: the entry names the same concrete version, or it is a range the
 * wanted version satisfies.
 *
 * The wanted specifier has to be a concrete version. A wanted range is never covered, because the
 * catalog — not the dependency — decides which version a `catalog:` reference resolves to.
 */
function catalogCovers (catalogSpecifier: string, bareSpecifier: string | undefined): boolean {
  return bareSpecifier != null &&
    semver.valid(bareSpecifier) != null &&
    semver.validRange(catalogSpecifier) != null &&
    semver.satisfies(bareSpecifier, catalogSpecifier)
}

/**
 * Determines the catalog name for a dependency during installSome.
 *
 * If the dependency's previous specifier already uses a named catalog
 * (e.g. "catalog:foo"), that catalog name takes priority over the global
 * saveCatalogName option. This ensures that interactive updates and
 * `--latest` upgrades preserve the per-dependency catalog group.
 */
function getPerDepCatalogName (
  wantedDep: { prevSpecifier?: string },
  globalSaveCatalogName: string | undefined
): string {
  if (wantedDep.prevSpecifier) {
    const catalogFromPrev = parseCatalogProtocol(wantedDep.prevSpecifier)
    if (catalogFromPrev != null) {
      return catalogFromPrev
    }
  }
  return globalSaveCatalogName ?? 'default'
}

function isExplicitDistTagSpecifier (bareSpecifier: string | undefined): boolean {
  return bareSpecifier != null && bareSpecifier !== 'latest' && !bareSpecifier.includes(':') && semver.validRange(bareSpecifier) == null
}
