import type { ProjectManifest } from '@pnpm/types'
import { tryReadProjectManifest } from '@pnpm/workspace.project-manifest-reader'

/**
 * Copies the workspace root's `packageManager` field into a deployed manifest
 * that pins no package manager itself.
 *
 * The root's `devEngines` is never copied. It is a development-time contract
 * that package managers enforce by default, and npm refuses to run any script
 * of a manifest whose `devEngines.packageManager` names another package
 * manager.
 */
export function inheritPackageManager (
  manifest: ProjectManifest,
  enginePinManifest: ProjectManifest | undefined
): ProjectManifest {
  const packageManager = enginePinManifest?.packageManager
  if (
    packageManager == null ||
    manifest.packageManager != null ||
    manifest.devEngines?.packageManager != null
  ) {
    return manifest
  }
  return { ...manifest, packageManager }
}

export async function writeInheritedPackageManager (
  deployDir: string,
  enginePinManifest: ProjectManifest | undefined
): Promise<void> {
  const { manifest, writeProjectManifest } = await tryReadProjectManifest(deployDir)
  if (manifest == null) return
  const inherited = inheritPackageManager(manifest, enginePinManifest)
  if (inherited !== manifest) {
    await writeProjectManifest(inherited)
  }
}
