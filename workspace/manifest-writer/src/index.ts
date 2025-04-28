import fs from 'fs'
import path from 'path'
import { readWorkspaceManifest, type WorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import writeYamlFile from 'write-yaml-file'
import equals from 'ramda/src/equals'

async function writeManifestFile (dir: string, manifest: WorkspaceManifest): Promise<void> {
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

export async function addDefaultCatalog (workspaceDir: string, name: string, spec: string): Promise<void> {
  const manifest: WorkspaceManifest = await readWorkspaceManifest(workspaceDir) ?? {
    packages: [],
  }

  let catalog: Record<string, string> | undefined = manifest.catalog ?? manifest.catalogs?.default
  if (catalog == null) {
    catalog = {}
    manifest.catalog = catalog
  }

  catalog[name] = spec

  await writeManifestFile(workspaceDir, manifest)
}
