import { type Catalogs } from '@pnpm/catalogs.types'
import { readWorkspaceManifest } from '@pnpm/workspace.read-manifest'

export async function readCatalogsFromWorkspaceManifest (
  workspaceDir: string,
  isCatalogsFeatureFlagEnabled: boolean
): Promise<Catalogs> {
  const workspaceManifest = await readWorkspaceManifest(workspaceDir, { catalogs: isCatalogsFeatureFlagEnabled })

  if (workspaceManifest?.catalog == null && workspaceManifest?.catalogs == null) {
    return {}
  }

  return {
    // The readWorkspaceManifest function validates that the default catalog is
    // specified using only the "catalog" field or as a named catalog under the
    // catalogs block, but not both.
    //
    // If workspaceManifest.catalog is undefined, intentionally allow the spread
    // below to overwrite it.
    default: workspaceManifest.catalog,

    ...workspaceManifest.catalogs,
  }
}
