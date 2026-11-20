import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import type { Catalogs } from '@pnpm/catalogs.types'
import type { VersionOverride } from '@pnpm/config.parse-overrides'
import type {
  CatalogSnapshots,
  LockfileObject,
  ProjectSnapshot,
  ResolvedCatalogEntry,
  ResolvedDependencies,
} from '@pnpm/lockfile.types'
import semver from 'semver'

import { catalogReferencesHaveSnapshots, ownSpecifier } from './tryFastUpdateCatalogs.js'
import {
  applyFastRewrite,
  type FastOverride,
  type FastRewriteOptions,
} from './tryFastUpdateOverrides.js'

/**
 * Move a catalog entry to a version the lockfile does not have, without
 * resolving the whole graph.
 *
 * Replacing the package is the rewrite an exact `pnpm.overrides` entry
 * performs, so this drives that machinery rather than repeating it;
 * `tryFastUpdateCatalogs` keeps the range-only case, where the specifier moves
 * and the package does not.
 *
 * An override moves a package everywhere it appears. A catalog entry governs
 * only the importers that reference it, so anything else reaching the package
 * would have to keep the old version while those importers move — a graph
 * holding both, which this cannot express.
 */
export type CatalogVersionRewrite =
  /** Every entry that moved was rewritten. */
  | 'applied'
  /** No entry moved to a version the locked one cannot satisfy. */
  | 'nothing-to-move'
  /** An entry moved but the rewrite cannot express it; the resolver must run. */
  | 'unsupported'

export async function tryFastUpdateCatalogVersions (
  lockfile: LockfileObject,
  opts: FastRewriteOptions & { catalogs: Catalogs, parsedOverrides: VersionOverride[] }
): Promise<CatalogVersionRewrite> {
  // The same gate the range-only path opens with: an importer pointing at a
  // catalog entry with nothing recorded for it needs the resolver, and this
  // path would otherwise never look at that entry.
  if (!catalogReferencesHaveSnapshots(lockfile, opts.catalogs)) return 'unsupported'
  if (lockfile.catalogs == null) return 'nothing-to-move'
  const plan = planCatalogMoves(lockfile.catalogs, lockfile, opts)
  if (plan == null) return 'unsupported'
  if (plan.fastOverrides.length === 0) return 'nothing-to-move'

  return await applyFastRewrite(lockfile, plan.fastOverrides, opts, { catalogs: plan.catalogs })
    ? 'applied'
    : 'unsupported'
}

interface CatalogMovesPlan {
  catalogs: CatalogSnapshots
  fastOverrides: FastOverride[]
}

interface CatalogEntryPlan {
  entry: ResolvedCatalogEntry
  move?: FastOverride
}

interface CatalogPlanOptions {
  catalogs: Catalogs
  parsedOverrides: VersionOverride[]
}

/** The catalog snapshots the configuration asks for, and the package moves they need; `null` when one is unsupported. */
function planCatalogMoves (
  lockfileCatalogs: CatalogSnapshots,
  lockfile: LockfileObject,
  opts: CatalogPlanOptions
): CatalogMovesPlan | null {
  const fastOverrides: FastOverride[] = []
  const catalogs: CatalogSnapshots = {}
  for (const [catalogName, catalog] of Object.entries(lockfileCatalogs)) {
    catalogs[catalogName] = {}
    for (const [alias, entry] of Object.entries(catalog)) {
      const entryPlan = planCatalogEntry(lockfile, { catalogName, alias, entry }, opts)
      if (entryPlan == null) return null
      catalogs[catalogName][alias] = entryPlan.entry
      if (entryPlan.move != null) fastOverrides.push(entryPlan.move)
    }
  }
  return { catalogs, fastOverrides }
}

function planCatalogEntry (
  lockfile: LockfileObject,
  { catalogName, alias, entry }: { catalogName: string, alias: string, entry: ResolvedCatalogEntry },
  opts: CatalogPlanOptions
): CatalogEntryPlan | null {
  const specifier = opts.catalogs[catalogName]?.[alias]
  if (specifier == null) return null
  if (specifier === entry.specifier) return { entry }
  if (semver.valid(entry.version) == null) return null
  // A specifier the locked version still satisfies moves nothing but the
  // specifier, exactly as the range-only path would.
  if (semver.validRange(specifier) != null && semver.satisfies(entry.version, specifier)) {
    return { entry: { specifier, version: entry.version } }
  }
  const wanted = semver.valid(specifier)
  if (wanted == null) return null
  if (!catalogEntryIsSoleReference(lockfile, catalogName, alias)) return null
  if (isOverridden(alias, opts.parsedOverrides)) return null
  return {
    entry: { specifier, version: wanted },
    move: { name: alias, newVersion: wanted, oldVersion: entry.version },
  }
}

/**
 * Whether an override decides `alias`'s version, making the catalog specifier
 * no longer the last word on it. A `parent>child` selector names no importer
 * edge, and only importer edges are what a catalog entry resolves.
 */
function isOverridden (alias: string, parsedOverrides: VersionOverride[]): boolean {
  return parsedOverrides.some((override) =>
    override.parentPkg == null && override.targetPkg.name === alias)
}

/** Whether the catalog entry is the only thing in the lockfile that reaches `alias`. */
function catalogEntryIsSoleReference (
  lockfile: LockfileObject,
  catalogName: string,
  alias: string
): boolean {
  const importersAgree = Object.values(lockfile.importers).every((importer: ProjectSnapshot) =>
    dependencyGroups(importer).every((dependencies) =>
      !Object.hasOwn(dependencies, alias) ||
      dependencies[alias] == null ||
      parseCatalogProtocol(ownSpecifier(importer, alias)) === catalogName
    )
  )
  const noPackageDependsOnIt = Object.values(lockfile.packages ?? {}).every((snapshot) =>
    snapshot.dependencies?.[alias] == null && snapshot.optionalDependencies?.[alias] == null
  )
  return importersAgree && noPackageDependsOnIt
}

function dependencyGroups (importer: ProjectSnapshot): ResolvedDependencies[] {
  return [
    importer.dependencies,
    importer.devDependencies,
    importer.optionalDependencies,
  ].filter((group) => group != null)
}
