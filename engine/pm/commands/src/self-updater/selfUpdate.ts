import fs from 'node:fs'
import path from 'node:path'

import { linkBins } from '@pnpm/bins.linker'
import { isExecutedByCorepack, packageManager } from '@pnpm/cli.meta'
import { docsUrl } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, parsePackageManager, types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { createResolver } from '@pnpm/installing.client'
import { resolvePackageManagerIntegrities } from '@pnpm/installing.env-installer'
import { readEnvLockfile } from '@pnpm/lockfile.fs'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { whichVersionIsPinned } from '@pnpm/resolving.npm-resolver'
import { createStoreController, type CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import type { PinnedVersion } from '@pnpm/types'
import { readProjectManifest } from '@pnpm/workspace.project-manifest-reader'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'
import semver from 'semver'

import { installPnpm } from './installPnpm.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
  }
}

export const commandNames = ['self-update']

// Migration guidance printed once when `pnpm self-update` crosses a major
// boundary. Add an entry here for each future major that ships breaking
// changes users need to act on.
const MAJOR_UPGRADE_HINTS: Record<number, string> = {
  11:
    'pnpm v11 removed or renamed several v10 settings. ' +
    'See https://pnpm.io/11.x/migration for migration instructions.',
}

export const skipPackageManagerCheck = true

export function help (): string {
  return renderHelp({
    description: 'Updates pnpm to the latest version (or the one specified)',
    descriptionLists: [],
    url: docsUrl('self-update'),
    usages: [
      'pnpm self-update',
      'pnpm self-update 9',
      'pnpm self-update next-10',
      'pnpm self-update 9.10.0',
    ],
  })
}

export type SelfUpdateCommandOptions = CreateStoreControllerOptions & Pick<Config,
| 'globalPkgDir'
| 'lockfileDir'
| 'modulesDir'
| 'pnpmHomeDir'
> & Pick<ConfigContext,
| 'rootProjectManifestDir'
| 'wantedPackageManager'
>

export async function handler (
  opts: SelfUpdateCommandOptions,
  params: string[]
): Promise<undefined | string> {
  if (isExecutedByCorepack()) {
    throw new PnpmError('CANT_SELF_UPDATE_IN_COREPACK', 'You should update pnpm with corepack')
  }
  globalInfo('Checking for updates...')
  const { resolve } = createResolver({ ...opts, configByUri: opts.configByUri })
  const pkgName = 'pnpm'
  // `pnpm self-update` (no args) defaults to the `latest` dist-tag, but we
  // refuse to downgrade in that case — `latest` on the registry can lag the
  // installed version when a new major has shipped without being tagged.
  // `pnpm self-update latest` (explicit) bypasses the guard so users can
  // still force a downgrade when they want one.
  const isImplicitLatest = params.length === 0
  const bareSpecifier = params[0] ?? 'latest'
  const resolution = await resolve({ alias: pkgName, bareSpecifier }, {
    lockfileDir: opts.lockfileDir ?? opts.dir,
    preferredVersions: {},
    projectDir: opts.dir,
  })
  if (!resolution?.manifest) {
    throw new PnpmError('CANNOT_RESOLVE_PNPM', `Cannot find "${bareSpecifier}" version of pnpm`)
  }

  // Determine the "previous" pnpm version being upgraded FROM. If the
  // project pins pnpm via `packageManager`/`devEngines.packageManager`,
  // the pin is the source of truth — the running pnpm binary may already
  // be at a newer major (e.g. a globally-installed v11 operating on a
  // project still pinned to v10). Otherwise fall back to the running
  // binary. Skip the hint entirely on a no-op (target === previous).
  const targetVersion = resolution.manifest.version
  let previousVersion: string | undefined
  if (opts.wantedPackageManager?.name === packageManager.name) {
    if (opts.wantedPackageManager.version !== targetVersion) {
      previousVersion = opts.wantedPackageManager.version
    }
  } else if (packageManager.version !== targetVersion) {
    previousVersion = packageManager.version
  }
  const previousMajor = previousVersion != null
    ? semver.coerce(previousVersion)?.major
    : undefined
  const targetMajor = semver.major(targetVersion)
  if (previousMajor != null && targetMajor > previousMajor) {
    const hint = MAJOR_UPGRADE_HINTS[targetMajor]
    if (hint) globalWarn(hint)
  }

  if (opts.wantedPackageManager?.name === packageManager.name) {
    if (opts.wantedPackageManager?.version !== resolution.manifest.version) {
      if (isImplicitLatest) {
        // Prefer the lockfile-pinned version when available — for range
        // specs like `>=8.0.0`, the spec's lower bound understates the
        // version that was actually installed (see #11418 review).
        const projectCurrentVersion = await readProjectPinnedPnpmVersion(opts.rootProjectManifestDir, opts.wantedPackageManager?.version)
        if (projectCurrentVersion != null && semver.lt(resolution.manifest.version, projectCurrentVersion)) {
          return `The current project is set to use pnpm v${projectCurrentVersion}, which is newer than the "latest" version on the registry (v${resolution.manifest.version}). No update performed. Run "pnpm self-update latest" to downgrade.`
        }
      }
      const { manifest, writeProjectManifest } = await readProjectManifest(opts.rootProjectManifestDir)
      if (manifest.devEngines?.packageManager) {
        let manifestChanged = false
        // If "packageManager" pins pnpm, treat both fields as the user's
        // single source of truth for the active pnpm version: rewrite both
        // to the new exact version (dropping any range operator in
        // devEngines and any integrity hash on the legacy field). When only
        // devEngines is set, preserve the user's range style and let the
        // lockfile pin the exact version.
        const legacyPm = manifest.packageManager != null
          ? parsePackageManager(manifest.packageManager)
          : undefined
        const legacyPinsPnpm = legacyPm?.name === 'pnpm' && legacyPm.version != null
        const devEnginesPm = manifest.devEngines.packageManager
        const pnpmEntry = Array.isArray(devEnginesPm)
          ? devEnginesPm.find((e) => e.name === 'pnpm')
          : devEnginesPm.name === 'pnpm' ? devEnginesPm : undefined
        if (pnpmEntry) {
          const updated = legacyPinsPnpm
            ? resolution.manifest.version
            : updateVersionConstraint(pnpmEntry.version, resolution.manifest.version)
          if (updated !== pnpmEntry.version) {
            pnpmEntry.version = updated
            manifestChanged = true
          }
        }
        if (legacyPinsPnpm) {
          const newLegacy = `pnpm@${resolution.manifest.version}`
          if (manifest.packageManager !== newLegacy) {
            manifest.packageManager = newLegacy
            manifestChanged = true
          }
        }
        if (manifestChanged) await writeProjectManifest(manifest)
        const store = await createStoreController(opts)
        await resolvePackageManagerIntegrities(resolution.manifest.version, {
          registries: opts.registries,
          rootDir: opts.rootProjectManifestDir,
          storeController: store.ctrl,
          storeDir: store.dir,
        })
      } else {
        manifest.packageManager = `pnpm@${resolution.manifest.version}`
        await writeProjectManifest(manifest)
      }
      return `The current project has been updated to use pnpm v${resolution.manifest.version}`
    } else {
      return `The current project is already set to use pnpm v${resolution.manifest.version}`
    }
  }
  if (resolution.manifest.version === packageManager.version) {
    return `The currently active ${packageManager.name} v${packageManager.version} is already "${bareSpecifier}" and doesn't need an update`
  }

  if (isImplicitLatest && semver.lt(resolution.manifest.version, packageManager.version)) {
    return `The currently active ${packageManager.name} v${packageManager.version} is newer than the "latest" version on the registry (v${resolution.manifest.version}). No update performed. Run "pnpm self-update latest" to downgrade.`
  }

  globalInfo(`Updating pnpm from v${packageManager.version} to v${resolution.manifest.version}...`)
  const store = await createStoreController(opts)

  // Resolve integrities and write env lockfile to pnpm-lock.yaml
  const envLockfile = await resolvePackageManagerIntegrities(resolution.manifest.version, {
    registries: opts.registries,
    rootDir: opts.pnpmHomeDir,
    storeController: store.ctrl,
    storeDir: store.dir,
  })

  const { baseDir, alreadyExisted } = await installPnpm(resolution.manifest.version, {
    ...opts,
    envLockfile,
    storeController: store.ctrl,
    storeDir: store.dir,
  })

  // Link bins to pnpmHomeDir/bin so the updated pnpm is the active global binary
  await linkBins(path.join(baseDir, 'node_modules'), path.join(opts.pnpmHomeDir, 'bin'), { warn: globalWarn })

  // pnpm v10 setup linked bins directly into pnpmHomeDir and added that
  // directory to PATH (instead of pnpmHomeDir/bin as v11 does). When a v10
  // user upgrades to v11 the legacy shims at pnpmHomeDir keep pointing into
  // the old `.tools/<version>` install — so PATH still resolves `pnpm` to the
  // pre-update version. Detect that case and refresh the legacy shims so the
  // upgrade actually takes effect, then warn the user to run `pnpm setup`
  // for a clean migration to the v11 layout. See pnpm/pnpm#11464.
  if (hasLegacyHomeDirShim(opts.pnpmHomeDir)) {
    await linkBins(path.join(baseDir, 'node_modules'), opts.pnpmHomeDir, { warn: globalWarn })
    globalWarn(
      'Detected a pnpm v10 installation layout at PNPM_HOME. The pnpm shims ' +
      'at PNPM_HOME have been refreshed so the new version is active, but ' +
      'pnpm v11 expects bins in PNPM_HOME/bin. Run "pnpm setup" to migrate ' +
      'your PATH to the v11 layout.'
    )
  }

  if (alreadyExisted) {
    return `The ${bareSpecifier} version, v${resolution.manifest.version}, is already present on the system. It was activated by linking it from ${baseDir}.`
  }
  return `Successfully updated pnpm to v${resolution.manifest.version}`
}

// A fresh v11 setup never writes a `pnpm` shim at pnpmHomeDir itself — only
// under pnpmHomeDir/bin. The presence of a `pnpm` (or `pnpm.cmd`) file
// directly at pnpmHomeDir is therefore a reliable v10-layout marker.
function hasLegacyHomeDirShim (pnpmHomeDir: string): boolean {
  for (const name of ['pnpm', 'pnpm.cmd']) {
    if (fs.existsSync(path.join(pnpmHomeDir, name))) return true
  }
  return false
}

/**
 * Returns the updated version constraint for devEngines.packageManager.
 * - Exact versions and simple ranges (^, ~) are updated to the new version,
 *   preserving the range operator.
 * - Ranges that still satisfy the new version are returned unchanged
 *   (the exact version will be pinned in the lockfile instead).
 * - Complex ranges (>=x <y, etc.) that no longer satisfy the new version
 *   fall back to a caret range with the new version (`^${newVersion}`).
 */
function updateVersionConstraint (current: string | undefined, newVersion: string): string | undefined {
  if (current == null) return newVersion
  // Range that still satisfies the new version — leave it as-is (lockfile handles pinning)
  if (semver.satisfies(newVersion, current, { includePrerelease: true })) return current
  // Determine the pinning style of the current specifier
  const pinnedVersion = whichVersionIsPinned(current)
  if (pinnedVersion == null) {
    // Complex range that can't be updated while preserving its structure — fall back to ^version
    return `^${newVersion}`
  }
  return versionSpecFromPinned(newVersion, pinnedVersion)
}

function versionSpecFromPinned (version: string, pinnedVersion: PinnedVersion): string {
  switch (pinnedVersion) {
    case 'none':
    case 'major': return `^${version}`
    case 'minor': return `~${version}`
    case 'patch': return version
  }
}

async function readProjectPinnedPnpmVersion (rootProjectManifestDir: string, spec: string | undefined): Promise<string | undefined> {
  // The env lockfile is the most accurate source for the actually-installed
  // pnpm version when the spec is a range. Fall back to the spec's minimum
  // version when there's no lockfile entry (e.g. exact `packageManager` pins
  // below v12 don't write to the lockfile). Take the max of the two so we
  // pick whichever signal is more restrictive.
  let lockfilePinned: string | undefined
  try {
    const envLockfile = await readEnvLockfile(rootProjectManifestDir)
    lockfilePinned = envLockfile?.importers['.'].packageManagerDependencies?.pnpm?.version
  } catch {
    // ignore — fall through to spec min version
  }
  let specMin: string | undefined
  if (spec != null) {
    try {
      specMin = semver.minVersion(spec)?.version
    } catch {
      // invalid range — ignore
    }
  }
  if (lockfilePinned != null && specMin != null) {
    return semver.gt(lockfilePinned, specMin) ? lockfilePinned : specMin
  }
  return lockfilePinned ?? specMin
}
