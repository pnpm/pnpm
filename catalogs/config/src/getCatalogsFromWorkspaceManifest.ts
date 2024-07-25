import { PnpmError } from '@pnpm/error'
import { type Catalogs } from '@pnpm/catalogs.types'
import { type WorkspaceManifest } from '@pnpm/workspace.read-manifest'

export function getCatalogsFromWorkspaceManifest (
  workspaceManifest: Pick<WorkspaceManifest, 'catalog' | 'catalogs'> | undefined
): Catalogs {
  // If the pnpm-workspace.yaml file doesn't exist, no catalogs are defined.
  //
  // In some cases, it makes sense for callers to handle null/undefined checks
  // of this form. In this case, let's explicitly handle not found
  // pnpm-workspace.yaml files by returning an empty catalog to make consuming
  // logic easier.
  if (workspaceManifest == null) {
    return {}
  }

  checkDefaultCatalogIsDefinedOnce(workspaceManifest)

  return {
    // If workspaceManifest.catalog is undefined, intentionally allow the spread
    // below to overwrite it. The check above ensures only one or the either is
    // defined.
    default: workspaceManifest.catalog,

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
