import fs from 'fs'
import path from 'path'
import { type ResolvedCatalogEntry } from '@pnpm/lockfile.types'
import { readWorkspaceManifest, type WorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import writeYamlFile from 'write-yaml-file'
import equals from 'ramda/src/equals'
import { sortKeysByPriority } from '@pnpm/object.key-sorting'

async function writeManifestFile (dir: string, manifest: Partial<WorkspaceManifest>): Promise<void> {
  manifest = sortKeysByPriority({
    priority: { packages: 0 },
    deep: false,
  }, manifest)
  return writeYamlFile(path.join(dir, WORKSPACE_MANIFEST_FILENAME), manifest, {
    lineWidth: -1, // This is setting line width to never wrap
    blankLines: true,
    noCompatMode: true,
    noRefs: true,
    sortKeys: false,
  })
}

export async function updateWorkspaceManifest (dir: string, updatedFields: Partial<WorkspaceManifest>): Promise<void> {
  const manifest = await readWorkspaceManifest(dir) ?? {} as WorkspaceManifest
  let shouldBeUpdated = false
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
  await writeManifestFile(dir, manifest)
}

export async function addDefaultCatalogs (
  workspaceDir: string,
  newDefaultCatalogs: Record<string, Pick<ResolvedCatalogEntry, 'specifier'>>
): Promise<void> {
  const manifest: Partial<WorkspaceManifest> = await readWorkspaceManifest(workspaceDir) ?? {}

  let targetCatalog: Record<string, string> | undefined = manifest.catalog ?? manifest.catalogs?.default
  const targetCatalogWasNil = targetCatalog == null

  for (const alias in newDefaultCatalogs) {
    targetCatalog ??= {}
    targetCatalog[alias] = newDefaultCatalogs[alias].specifier
  }

  if (targetCatalogWasNil) {
    manifest.catalog = targetCatalog
  }

  await writeManifestFile(workspaceDir, manifest)
}
