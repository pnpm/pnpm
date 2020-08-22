import path = require('path')
import findUp = require('find-up')

const WORKSPACE_MANIFEST_FILENAME = 'pnpm-workspace.yaml'

export default async function findWorkspaceDir (cwd: string) {
  const workspaceManifestLocation = await findUp(WORKSPACE_MANIFEST_FILENAME, { cwd })
  return workspaceManifestLocation && path.dirname(workspaceManifestLocation)
}
