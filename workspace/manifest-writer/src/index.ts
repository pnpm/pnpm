import path from 'path'
import { readWorkspaceManifest, type WorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import writeYamlFile from 'write-yaml-file'
import equals from 'ramda/src/equals'

export async function updateWorkspaceManifest (dir: string, updatedFields: Partial<WorkspaceManifest>): Promise<void> {
  const manifest = await readWorkspaceManifest(dir)
  let shouldBeUpdated = false
  if (manifest != null) {
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
  } else {
    shouldBeUpdated = true
  }
  if (shouldBeUpdated) {
    await writeYamlFile(path.join(dir, WORKSPACE_MANIFEST_FILENAME), {
      ...manifest,
      ...updatedFields,
    })
  }
}
