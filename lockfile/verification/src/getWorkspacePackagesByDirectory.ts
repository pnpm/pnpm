import { type WorkspacePackages } from '@pnpm/resolver-base'
import { type DependencyManifest } from '@pnpm/types'

export function getWorkspacePackagesByDirectory (workspacePackages: WorkspacePackages): Record<string, DependencyManifest> {
  const workspacePackagesByDirectory: Record<string, DependencyManifest> = {}
  if (workspacePackages) {
    for (const pkgVersions of workspacePackages.values()) {
      for (const { rootDir, manifest } of pkgVersions.values()) {
        workspacePackagesByDirectory[rootDir] = manifest
      }
    }
  }
  return workspacePackagesByDirectory
}
