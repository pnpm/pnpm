import { readWorkspaceManifest, type WorkspaceManifest } from '@pnpm/workspace.read-manifest'
import writeYamlFile from 'write-yaml-file'

export async function updateWorkspaceManifest (dir: string, updatedFields: Partial<WorkspaceManifest>): Promise<void> {
  const manifest = await readWorkspaceManifest(dir)
  await writeYamlFile(dir, {
    ...manifest,
    updatedFields,
  })
}
