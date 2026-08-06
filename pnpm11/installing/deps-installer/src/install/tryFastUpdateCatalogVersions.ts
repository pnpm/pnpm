import { parseCatalogProtocol } from '@pnpm/catalogs.protocol-parser'
import type { Catalogs } from '@pnpm/catalogs.types'
import type { LockfileObject, ProjectSnapshot, ResolvedDependencies } from '@pnpm/lockfile.types'
import semver from 'semver'

import {
  applyFastRewrite,
  type FastOverride,
  type FastRewriteOptions,
} from './tryFastUpdateOverrides.js'

/**
 * Move a catalog entry to a version the lockfile does not have, without
 * resolving the whole graph.
 *
 * `tryFastUpdateCatalogs` handles the range-only case, where the recorded
 * version still satisfies the new specifier and nothing but the specifier
 * moves. This handles the other half: the entry now names a version the
 * locked one cannot satisfy, so the package itself has to be replaced. That
 * is the same rewrite an exact `pnpm.overrides` entry performs, so it reuses
 * that machinery — including the check that every locked child still
 * satisfies the new version's manifest.
 *
 * Unlike an override, a catalog entry only governs the importers that
 * reference it. Returns `false` when anything else reaches the package, since
 * the graph would then have to hold both versions.
 */
export async function tryFastUpdateCatalogVersions (
  lockfile: LockfileObject,
  opts: FastRewriteOptions & { catalogs: Catalogs }
): Promise<boolean> {
  if (lockfile.catalogs == null) return false
  const fastOverrides: FastOverride[] = []
  const catalogs: LockfileObject['catalogs'] = {}
  for (const [catalogName, catalog] of Object.entries(lockfile.catalogs)) {
    catalogs[catalogName] = {}
    for (const [alias, entry] of Object.entries(catalog)) {
      const specifier = opts.catalogs[catalogName]?.[alias]
      if (specifier == null) return false
      if (specifier === entry.specifier) {
        catalogs[catalogName][alias] = entry
        continue
      }
      // A specifier the locked version still satisfies belongs to the
      // range-only path, which rewrites nothing.
      const wanted = semver.valid(specifier)
      if (wanted == null || semver.valid(entry.version) == null || wanted === entry.version) {
        return false
      }
      if (!catalogEntryIsSoleReference(lockfile, catalogName, alias)) return false
      fastOverrides.push({ name: alias, newVersion: wanted, oldVersion: entry.version })
      catalogs[catalogName][alias] = { specifier, version: wanted }
    }
  }
  if (fastOverrides.length === 0) return false

  return applyFastRewrite(lockfile, fastOverrides, opts, { catalogs })
}

/**
 * Whether the catalog entry is the only thing in the lockfile that reaches
 * `alias`: every importer that depends on it does so through this catalog,
 * and no package depends on it at all.
 *
 * An override moves a package everywhere it appears. A catalog entry cannot,
 * so anything else pointing at the package would have to keep the old version
 * while the catalog's importers move to the new one, leaving the graph holding
 * both.
 */
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
