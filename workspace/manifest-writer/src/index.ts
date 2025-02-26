import { readWorkspaceManifest, type WorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import writeYamlFile from 'write-yaml-file'
import path from 'node:path'

export async function updateWorkspaceManifest (dir: string, updatedFields: Partial<WorkspaceManifest>): Promise<void> {
  const manifest = await readWorkspaceManifest(dir)
  await writeYamlFile(path.join(dir, WORKSPACE_MANIFEST_FILENAME), {
    ...manifest,
    ...updatedFields,
  })
}
