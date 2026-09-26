import path from 'node:path'

import type { WorkspacePackages } from '@pnpm/resolving.resolver-base'
import type { DependencyManifest } from '@pnpm/types'

export function getWorkspacePackagesByDirectory (workspacePackages: WorkspacePackages): Record<string, DependencyManifest> {
  const workspacePackagesByDirectory: Record<string, DependencyManifest> = {}
  if (workspacePackages) {
    for (const pkgVersions of workspacePackages.values()) {
      for (const { rootDir, manifest } of pkgVersions.values()) {
        workspacePackagesByDirectory[rootDir] = manifest
        // A project whose `publishConfig.directory` is injected is packed
        // from that directory rather than its root (see
        // `resolveLocalPackageDir` in `@pnpm/resolving.npm-resolver`), and
        // that directory doesn't exist yet on a fresh checkout, before the
        // project's own build script runs. Key the manifest under it too so
        // an up-to-date check for such a dependency doesn't read from disk
        // at a directory the coming install step is about to create.
        const publishDir = manifest.publishConfig?.directory
        if (publishDir != null && manifest.publishConfig?.linkDirectory !== false) {
          workspacePackagesByDirectory[path.join(rootDir, publishDir)] = manifest
        }
      }
    }
  }
  return workspacePackagesByDirectory
}
