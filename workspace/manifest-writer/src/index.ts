import fs from 'fs'
import path from 'path'
import { readWorkspaceManifest, type WorkspaceCatalog, type WorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import writeYamlFile from 'write-yaml-file'
import equals from 'ramda/src/equals'
import { type Catalog, type Catalogs } from '@pnpm/catalogs.types'
import { sortKeysByPriority } from '@pnpm/object.key-sorting'

export type WorkspaceManifestUpdater = Partial<WorkspaceManifest> | ((current: WorkspaceManifest) => Partial<WorkspaceManifest>)

export async function updateWorkspaceManifest (dir: string, updater: WorkspaceManifestUpdater): Promise<void> {
  let manifest = await readWorkspaceManifest(dir) ?? {} as WorkspaceManifest
  let shouldBeUpdated = false
  const updatedFields = typeof updater === 'function'
    ? updater(manifest)
    : updater
  for (const [key, value] of Object.entries(updatedFields)) {
    if (!equals(manifest[key as keyof WorkspaceManifest], value)) {
      shouldBeUpdated = true
      if (value == null) {
        delete manifest[key as keyof WorkspaceManifest]
      } else {
        // @ts-expect-error
        manifest[key as keyof WorkspaceManifest] = value
      }
    }
  }
  if (!shouldBeUpdated) {
    return
  }
  if (Object.keys(manifest).length === 0) {
    await fs.promises.rm(path.join(dir, WORKSPACE_MANIFEST_FILENAME))
    return
  }
  manifest = sortKeysByPriority({
    priority: { packages: 0 },
    deep: false,
  }, manifest)
  await writeYamlFile(path.join(dir, WORKSPACE_MANIFEST_FILENAME), manifest, {
    lineWidth: -1, // This is setting line width to never wrap
    noCompatMode: true,
    noRefs: true,
    sortKeys: false,
  })
}

/**
 * Accepts a partial of {@type Catalogs} and updates the current pnpm-workspace.yaml.
 */
export async function updateCatalogs (dir: string, catalogs: Catalogs): Promise<void> {
  return updateWorkspaceManifest(dir, (current): Pick<WorkspaceManifest, 'catalog' | 'catalogs'> => {
    const result: Pick<WorkspaceManifest, 'catalog' | 'catalogs'> = {}

    const { default: defaultCatalog, ...namedCatalogs } = catalogs

    const isDefaultCatalogShorthandUsed = Object.keys(current.catalogs?.default ?? {}).length > 0
    const newNamedCatalogs = updateNamedCatalogs(current.catalogs, isDefaultCatalogShorthandUsed ? namedCatalogs : catalogs)

    if (isDefaultCatalogShorthandUsed) {
      result.catalog = upsertWorkspaceCatalog(current.catalogs?.default ?? {}, defaultCatalog)
    }

    if (Object.keys(newNamedCatalogs ?? {}).length > 0) {
      result.catalogs = newNamedCatalogs
    }

    return result
  })
}

function updateNamedCatalogs (current: WorkspaceManifest['catalogs'], updates: Catalogs): WorkspaceManifest['catalogs'] {
  const result: Record<string, Record<string, string>> = {}

  for (const [catalogName, catalog] of Object.entries(current ?? {})) {
    result[catalogName] = upsertWorkspaceCatalog(catalog, updates[catalogName])
  }

  for (const [catalogName, catalog] of Object.entries(updates)) {
    result[catalogName] ??= upsertWorkspaceCatalog({}, catalog)
  }

  return result
}

function upsertWorkspaceCatalog (current: WorkspaceCatalog, upserts?: Catalog): WorkspaceCatalog {
  if (upserts === undefined) {
    return current
  }

  const result: Record<string, string> = {}

  for (const [alias, spec] of Object.entries(current)) {
    result[alias] = upserts[alias] ?? spec
  }

  for (const [alias, spec] of Object.entries(upserts)) {
    if (spec == null) {
      continue
    }

    result[alias] ??= spec
  }

  return result
}
