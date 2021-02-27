import path from 'path'
import PnpmError from '@pnpm/error'
import findUp from 'find-up'

const WORKSPACE_MANIFEST_FILENAME = 'pnpm-workspace.yaml'

export default async function findWorkspaceDir (cwd: string) {
  const workspaceManifestLocation = await findUp([WORKSPACE_MANIFEST_FILENAME, 'pnpm-workspace.yml'], { cwd })
  if (workspaceManifestLocation?.endsWith('.yml')) {
    throw new PnpmError('BAD_WORKSPACE_MANIFEST_NAME', `The workspace manifest file should be named "pnpm-workspace.yaml". File found: ${workspaceManifestLocation}`)
  }
  return workspaceManifestLocation && path.dirname(workspaceManifestLocation)
}
