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
 * Only `packageManager` is inherited, never `devEngines.packageManager`: that
 * field is a development-time contract package managers enforce by default —
 * npm exits with `EBADDEVENGINES` before running any script of a manifest whose
 * entry names another package manager — so a deploy directory, which is
 * installed by whichever package manager its consumer uses, must not carry it.
 */
function inheritedPackageManager (enginePinManifest: ProjectManifest | undefined): string | undefined {
  return exactDevEnginesPin(enginePinManifest) ?? enginePinManifest?.packageManager
}

/**
 * The `<name>@<version>` spec the root's `devEngines.packageManager` pins to a
 * single version, or `undefined` when the root declares no such entry or pins a
 * range or a dist-tag.
 *
 * `devEngines.packageManager` outranks `packageManager`, so its pin is the one
 * the deploy directory should carry, as the `packageManager` field corepack
 * reads. A range names no version corepack could install, and a pin for another
 * package manager is not the project's, so both fall back to the field.
 */
function exactDevEnginesPin (enginePinManifest: ProjectManifest | undefined): string | undefined {
  const declared = enginePinManifest?.devEngines?.packageManager
  if (declared == null) return undefined
  // In array notation pnpm's own entry governs this CLI; without one, the first.
  const engines = Array.isArray(declared) ? declared : [declared]
  const engine = engines.find((engine) => engine.name === 'pnpm') ?? engines[0]
  if (engine?.version == null) return undefined
  const pinned = parsePackageManager(`${engine.name}@${engine.version}`)
  if (pinned.name !== engine.name || pinned.version == null) return undefined
  return validVersion(pinned.version) === pinned.version
    ? `${pinned.name}@${pinned.version}`
    : undefined
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
