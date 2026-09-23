import type { ProjectManifest } from '@pnpm/types'
import { tryReadProjectManifest } from '@pnpm/workspace.project-manifest-reader'

/**
 * Copies the workspace root's `packageManager` and `devEngines.packageManager`
 * into a deployed manifest that declares neither, so the deploy directory
 * pins the package manager the workspace uses.
 */
export function inheritPackageManager (
  manifest: ProjectManifest,
  workspaceRootManifest: ProjectManifest | undefined
): ProjectManifest {
  const packageManager = workspaceRootManifest?.packageManager
  const devEngines = workspaceRootManifest?.devEngines
  if (
    (packageManager == null && devEngines?.packageManager == null) ||
    manifest.packageManager != null ||
    manifest.devEngines?.packageManager != null
  ) {
    return manifest
  }
  const inherited = { ...manifest }
  if (packageManager != null) {
    inherited.packageManager = packageManager
  }
  if (devEngines?.packageManager != null) {
    inherited.devEngines = { ...manifest.devEngines, packageManager: devEngines.packageManager }
  }
  return inherited
}

export async function writeInheritedPackageManager (
  deployDir: string,
  workspaceRootManifest: ProjectManifest | undefined
): Promise<void> {
  const { manifest, writeProjectManifest } = await tryReadProjectManifest(deployDir)
  if (manifest == null) return
  const inherited = inheritPackageManager(manifest, workspaceRootManifest)
  if (inherited !== manifest) {
    await writeProjectManifest(inherited)
  }
}
