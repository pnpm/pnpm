import fs from 'node:fs'
import path from 'node:path'

import type { Catalogs } from '@pnpm/catalogs.types'
import { mergePackageVersionSpecs } from '@pnpm/config.version-policy'
import { type GLOBAL_CONFIG_YAML_FILENAME, WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import type { CatalogSnapshots, ResolvedCatalogEntry } from '@pnpm/lockfile.types'
import type {
  Project,
} from '@pnpm/types'
import { validateWorkspaceManifest, type WorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import { patchDocument } from '@pnpm/yaml.document-sync'
import { equals } from 'ramda'
import writeFileAtomic from 'write-file-atomic'
import yaml from 'yaml'

import { setAuditIgnoreGhsas } from './auditIgnoreGhsas.js'
import { addCatalogs, removePackagesFromWorkspaceCatalog } from './catalogs.js'
import { captureKeyOrder, propagateBlankLinesToNewPairs, reorderRecursive } from './keyOrder.js'
import { type ExcludeListField, pruneAllowBuilds, pruneExcludeList } from './pruneResolvedLists.js'

export type FileName =
  | typeof GLOBAL_CONFIG_YAML_FILENAME
  | typeof WORKSPACE_MANIFEST_FILENAME

const DEFAULT_FILENAME: FileName = WORKSPACE_MANIFEST_FILENAME

async function writeManifestFile (dir: string, fileName: FileName, manifest: yaml.Document): Promise<void> {
  const manifestStr = manifest.toString({
    lineWidth: 0, // This is setting line width to never wrap
    singleQuote: true, // Prefer single quotes over double quotes
  })
  await fs.promises.mkdir(dir, { recursive: true })
  await writeFileAtomic(path.join(dir, fileName), manifestStr)
}

async function readManifestRaw (file: string): Promise<string | undefined> {
  try {
    return (await fs.promises.readFile(file)).toString()
  } catch (err) {
    if (err != null && typeof err === 'object' && 'code' in err && err.code === 'ENOENT') {
      return undefined
    }
    throw err
  }
}

export interface UpdateWorkspaceManifestOptions {
  updatedFields?: Partial<WorkspaceManifest>
  updatedCatalogs?: Catalogs
  updatedOverrides?: Record<string, string>
  /**
   * The complete desired audit ignore list, written to whichever spelling
   * the manifest uses — see {@link setAuditIgnoreGhsas}. An empty array
   * removes the list.
   */
  updatedAuditIgnoreGhsas?: string[]
  addedMinimumReleaseAgeExcludes?: string[]
  deletedLegacyKeys?: string[]
  fileName?: FileName
  catalogPrune?: boolean
  /**
   * Lockfile catalogs whose entries `catalogPrune` keeps even when no project
   * references them.
   */
  keptCatalogs?: CatalogSnapshots
  allProjects?: Project[]
  /**
   * Package name → the versions the freshly resolved lockfile records.
   * Supplied when a freshly resolved shared lockfile is available.
   * `minimumReleaseAgeExcludePrune` and `trustPolicyExcludePrune` gate their
   * cleanups; `allowBuilds` cleanup runs whenever this map is present.
   */
  resolvedPackageVersions?: ReadonlyMap<string, ReadonlySet<string>>
  minimumReleaseAgeExcludePrune?: boolean
  trustPolicyExcludePrune?: boolean
}

export async function updateWorkspaceManifest (dir: string, opts: UpdateWorkspaceManifestOptions): Promise<void> {
  const fileName = opts.fileName ?? DEFAULT_FILENAME

  const workspaceManifestStr = await readManifestRaw(path.join(dir, fileName))

  const document = workspaceManifestStr != null
    ? yaml.parseDocument(workspaceManifestStr)
    : new yaml.Document()

  let manifest = document.toJSON()
  validateWorkspaceManifest(manifest)
  manifest ??= {}

  const originalKeyOrder = captureKeyOrder(manifest)

  if (!applyManifestUpdates(manifest, opts)) {
    return
  }
  if (Object.keys(manifest).length === 0) {
    await fs.promises.rm(path.join(dir, fileName))
    return
  }

  manifest = reorderRecursive(originalKeyOrder, manifest) as Partial<WorkspaceManifest>

  patchDocument(document, manifest, { preserveScalarAliases: true })
  propagateBlankLinesToNewPairs(document, originalKeyOrder?.keys ?? [])

  await writeManifestFile(dir, fileName, document)
}

export interface NewCatalogs {
  [catalogName: string]: {
    [dependencyName: string]: Pick<ResolvedCatalogEntry, 'specifier'>
  }
}

type WritableManifest = Partial<WorkspaceManifest> & { [key in ExcludeListField]?: string[] }

function applyManifestUpdates (manifest: WritableManifest, opts: UpdateWorkspaceManifestOptions): boolean {
  let shouldBeUpdated = applyCatalogUpdates(manifest, opts)
  shouldBeUpdated = applyUpdatedFields(manifest as Record<string, unknown>, { ...opts.updatedFields }) || shouldBeUpdated
  shouldBeUpdated = deleteKeys(manifest as Record<string, unknown>, opts.deletedLegacyKeys ?? []) || shouldBeUpdated
  if (opts.updatedOverrides) {
    shouldBeUpdated = applyUpdatedOverrides(manifest, opts.updatedOverrides) || shouldBeUpdated
  }
  if (opts.updatedAuditIgnoreGhsas != null) {
    shouldBeUpdated = setAuditIgnoreGhsas(manifest, opts.updatedAuditIgnoreGhsas) || shouldBeUpdated
  }
  if (opts.resolvedPackageVersions != null) {
    shouldBeUpdated = pruneResolvedPackageLists(manifest, opts, opts.resolvedPackageVersions) || shouldBeUpdated
  }
  // Merged after the cleanup pass so entries approved during this install
  // are never pruned by it in the same write.
  if (opts.addedMinimumReleaseAgeExcludes?.length) {
    shouldBeUpdated = addMinimumReleaseAgeExcludes(manifest, opts.addedMinimumReleaseAgeExcludes) || shouldBeUpdated
  }
  return shouldBeUpdated
}

function applyCatalogUpdates (manifest: WritableManifest, opts: UpdateWorkspaceManifestOptions): boolean {
  const catalogsAdded = opts.updatedCatalogs != null && addCatalogs(manifest, opts.updatedCatalogs)
  if (!opts.catalogPrune) {
    return catalogsAdded
  }
  return removePackagesFromWorkspaceCatalog(manifest, opts.allProjects ?? [], opts.keptCatalogs) || catalogsAdded
}

function applyUpdatedFields (manifest: Record<string, unknown>, updatedFields: Partial<WorkspaceManifest>): boolean {
  let shouldBeUpdated = false
  for (const [key, value] of Object.entries(updatedFields)) {
    if (value == null) {
      // Clearing a field the manifest never had changes nothing. Counting it as
      // an update would take the empty-manifest branch below and try to remove a
      // file that may not exist.
      if (!Object.hasOwn(manifest, key)) continue
      shouldBeUpdated = true
      delete manifest[key]
      continue
    }
    if (equals(manifest[key], value)) continue
    shouldBeUpdated = true
    manifest[key] = value
  }
  return shouldBeUpdated
}

function deleteKeys (manifest: Record<string, unknown>, keys: string[]): boolean {
  let shouldBeUpdated = false
  for (const key of keys) {
    if (Object.hasOwn(manifest, key)) {
      delete manifest[key]
      shouldBeUpdated = true
    }
  }
  return shouldBeUpdated
}

function applyUpdatedOverrides (manifest: Partial<WorkspaceManifest>, updatedOverrides: Record<string, string>): boolean {
  let shouldBeUpdated = false
  manifest.overrides ??= {}
  for (const [key, value] of Object.entries(updatedOverrides)) {
    if (!equals(manifest.overrides[key], value)) {
      shouldBeUpdated = true
      manifest.overrides[key] = value
    }
  }
  return shouldBeUpdated
}

function pruneResolvedPackageLists (
  manifest: WritableManifest,
  opts: Pick<UpdateWorkspaceManifestOptions, 'minimumReleaseAgeExcludePrune' | 'trustPolicyExcludePrune'>,
  resolvedPackageVersions: ReadonlyMap<string, ReadonlySet<string>>
): boolean {
  let shouldBeUpdated = false
  if (opts.minimumReleaseAgeExcludePrune) {
    shouldBeUpdated = pruneExcludeList(manifest, 'minimumReleaseAgeExclude', resolvedPackageVersions) || shouldBeUpdated
  }
  if (opts.trustPolicyExcludePrune) {
    shouldBeUpdated = pruneExcludeList(manifest, 'trustPolicyExclude', resolvedPackageVersions) || shouldBeUpdated
  }
  return pruneAllowBuilds(manifest, resolvedPackageVersions) || shouldBeUpdated
}

function addMinimumReleaseAgeExcludes (manifest: WritableManifest, addedExcludes: string[]): boolean {
  const existing = manifest.minimumReleaseAgeExclude ?? []
  const merged = mergePackageVersionSpecs([...existing, ...addedExcludes])
  if (equals(existing, merged)) {
    return false
  }
  manifest.minimumReleaseAgeExclude = merged
  return true
}
