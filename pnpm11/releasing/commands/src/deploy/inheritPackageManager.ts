import { parsePackageManager } from '@pnpm/config.reader'
import type { ProjectManifest } from '@pnpm/types'
import { tryReadProjectManifest } from '@pnpm/workspace.project-manifest-reader'
import { valid as validVersion } from 'semver'

/**
 * Copies the workspace root's package manager pin into a deployed manifest
 * that declares none, so the deploy directory pins the package manager the
 * workspace uses.
 */
export function inheritPackageManager (
  manifest: ProjectManifest,
  enginePinManifest: ProjectManifest | undefined
): ProjectManifest {
  if (manifest.packageManager != null || manifest.devEngines?.packageManager != null) {
    return manifest
  }
  const packageManager = inheritedPackageManager(enginePinManifest)
  if (packageManager == null) return manifest
  return { ...manifest, packageManager }
}

/**
 * The `packageManager` spec a deployed manifest should declare, for a workspace
 * root that declares one.
 *
 * Only `packageManager` is inherited, never `devEngines.packageManager`. That
 * field is a development-time contract package managers enforce by default.
 * npm exits with `EBADDEVENGINES` before running any script of a manifest whose
 * entry names another package manager, so a deploy directory, which is
 * installed by whichever package manager its consumer uses, must not carry it.
 *
 * `devEngines.packageManager` outranks `packageManager`, so an exact pnpm
 * version there becomes `pnpm@<version>`. A root `packageManager` naming that
 * same version is kept as written instead, so the corepack integrity hash it
 * may carry survives.
 */
function inheritedPackageManager (enginePinManifest: ProjectManifest | undefined): string | undefined {
  const declared = enginePinManifest?.packageManager
  const version = exactPnpmDevEnginesVersion(enginePinManifest)
  if (version == null) return declared
  if (declared != null) {
    const parsed = parsePackageManager(declared)
    if (parsed.name === 'pnpm' && parsed.version === version) return declared
  }
  return `pnpm@${version}`
}

/**
 * The version the root's pnpm `devEngines.packageManager` entry pins exactly,
 * with any integrity hash it carries, or `undefined` when the root has no pnpm
 * entry or it names a range or a dist-tag.
 *
 * Corepack installs the version named exactly, so a range names nothing it can
 * honor. An entry for another package manager is not the pin of a pnpm
 * workspace.
 */
function exactPnpmDevEnginesVersion (enginePinManifest: ProjectManifest | undefined): string | undefined {
  const declared = enginePinManifest?.devEngines?.packageManager
  if (declared == null) return undefined
  const engines = Array.isArray(declared) ? declared : [declared]
  const version = engines.find((engine) => engine.name === 'pnpm')?.version
  if (version == null) return undefined
  return validVersion(version) === version.split('+', 1)[0] ? version : undefined
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
