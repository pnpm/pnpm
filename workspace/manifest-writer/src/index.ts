import fs from 'fs'
import path from 'path'
import { readWorkspaceManifest, type WorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import writeYamlFile from 'write-yaml-file'
import equals from 'ramda/src/equals'
import { sortKeysByPriority } from '@pnpm/object.key-sorting'

export async function updateWorkspaceManifest (dir: string, updatedFields: Partial<WorkspaceManifest>): Promise<void> {
  let manifest = await readWorkspaceManifest(dir) ?? {} as WorkspaceManifest
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
