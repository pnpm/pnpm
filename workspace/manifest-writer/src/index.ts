import path from 'path'
import fs from 'fs'
import { readWorkspaceManifest, type WorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import writeYamlFile from 'write-yaml-file'

export async function updateWorkspaceManifest (dir: string, updatedFields: Partial<WorkspaceManifest>, isDelete?: boolean): Promise<void> {
  const manifest = await readWorkspaceManifest(dir)
  if (manifest && isDelete) {
    for (const field of Object.keys(updatedFields)) {
      delete manifest[field as keyof WorkspaceManifest]
    }
    if (Object.keys(manifest).length === 0) {
      try {
        fs.rmSync(path.join(dir, WORKSPACE_MANIFEST_FILENAME))
      } catch (err) {
      }
      return
    }
    await writeYamlFile(path.join(dir, WORKSPACE_MANIFEST_FILENAME), manifest)
    return
  }
  await writeYamlFile(path.join(dir, WORKSPACE_MANIFEST_FILENAME), {
    ...manifest,
    ...updatedFields,
  })
}
