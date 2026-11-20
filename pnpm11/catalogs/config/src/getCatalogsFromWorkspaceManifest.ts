import type { Catalogs } from '@pnpm/catalogs.types'
import { PnpmError } from '@pnpm/error'
import type { WorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'

export function getCatalogsFromWorkspaceManifest (
  workspaceManifest: Pick<WorkspaceManifest, 'catalog' | 'catalogs'> | undefined
): Catalogs {
  // If the pnpm-workspace.yaml file doesn't exist, no catalogs are defined.
  if (workspaceManifest == null) {
    return {}
  }

  checkDefaultCatalogIsDefinedOnce(workspaceManifest)

  return {
    ...(workspaceManifest.catalog != null ? { default: workspaceManifest.catalog } : {}),
    ...workspaceManifest.catalogs,
  }
}

export function checkDefaultCatalogIsDefinedOnce (manifest: Pick<WorkspaceManifest, 'catalog' | 'catalogs'>): void {
  if (manifest.catalog != null && manifest.catalogs?.default != null) {
    throw new PnpmError(
      'INVALID_CATALOGS_CONFIGURATION',
      'The \'default\' catalog was defined multiple times. Use the \'catalog\' field or \'catalogs.default\', but not both.')
  }
}
