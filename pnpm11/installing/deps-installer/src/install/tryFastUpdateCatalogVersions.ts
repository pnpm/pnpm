import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import type { Catalogs } from '@pnpm/catalogs.types'
import type { VersionOverride } from '@pnpm/config.parse-overrides'
import type { LockfileObject, ProjectSnapshot, ResolvedDependencies } from '@pnpm/lockfile.types'
import semver from 'semver'

import { catalogReferencesHaveSnapshots } from './tryFastUpdateCatalogs.js'
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
  const fastOverrides: FastOverride[] = []
  const catalogs: LockfileObject['catalogs'] = {}
  for (const [catalogName, catalog] of Object.entries(lockfile.catalogs)) {
    catalogs[catalogName] = {}
    for (const [alias, entry] of Object.entries(catalog)) {
      const specifier = opts.catalogs[catalogName]?.[alias]
      if (specifier == null) return 'unsupported'
      if (specifier === entry.specifier) {
        catalogs[catalogName][alias] = entry
        continue
      }
      if (semver.valid(entry.version) == null) return 'unsupported'
      // A specifier the locked version still satisfies moves nothing but the
      // specifier, exactly as the range-only path would.
      if (semver.validRange(specifier) != null && semver.satisfies(entry.version, specifier)) {
        catalogs[catalogName][alias] = { specifier, version: entry.version }
        continue
      }
      const wanted = semver.valid(specifier)
      if (wanted == null) return 'unsupported'
      if (!catalogEntryIsSoleReference(lockfile, catalogName, alias)) return 'unsupported'
      if (isOverridden(alias, opts.parsedOverrides)) return 'unsupported'
      fastOverrides.push({ name: alias, newVersion: wanted, oldVersion: entry.version })
      catalogs[catalogName][alias] = { specifier, version: wanted }
    }
  }
  if (fastOverrides.length === 0) return 'nothing-to-move'

  return await applyFastRewrite(lockfile, fastOverrides, opts, { catalogs })
    ? 'applied'
    : 'unsupported'
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
      dependencies[alias] == null ||
      parseCatalogProtocol(importer.specifiers[alias] ?? '') === catalogName
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
