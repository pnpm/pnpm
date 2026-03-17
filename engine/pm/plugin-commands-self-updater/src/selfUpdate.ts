import path from 'node:path'

import { isExecutedByCorepack, packageManager } from '@pnpm/cli-meta'
import { docsUrl } from '@pnpm/cli-utils'
import { createResolver } from '@pnpm/client'
import { type Config, types as allTypes } from '@pnpm/config'
import { resolvePackageManagerIntegrities } from '@pnpm/config.deps-installer'
import { PnpmError } from '@pnpm/error'
import { linkBins } from '@pnpm/link-bins'
import { globalWarn } from '@pnpm/logger'
import { whichVersionIsPinned } from '@pnpm/npm-resolver'
import { readProjectManifest } from '@pnpm/read-project-manifest'
import { createStoreController, type CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import type { PinnedVersion } from '@pnpm/types'
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
| 'managePackageManagerVersions'
| 'modulesDir'
| 'pnpmHomeDir'
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
  const { resolve } = createResolver({ ...opts, authConfig: opts.rawConfig })
  const pkgName = 'pnpm'
  const bareSpecifier = params[0] ?? 'latest'
  const resolution = await resolve({ alias: pkgName, bareSpecifier }, {
    lockfileDir: opts.lockfileDir ?? opts.dir,
    preferredVersions: {},
    projectDir: opts.dir,
  })
  if (!resolution?.manifest) {
    throw new PnpmError('CANNOT_RESOLVE_PNPM', `Cannot find "${bareSpecifier}" version of pnpm`)
  }

  if (opts.wantedPackageManager?.name === packageManager.name) {
    if (opts.wantedPackageManager?.version !== resolution.manifest.version) {
      const { manifest, writeProjectManifest } = await readProjectManifest(opts.rootProjectManifestDir)
      if (manifest.devEngines?.packageManager) {
        if (Array.isArray(manifest.devEngines.packageManager)) {
          const pnpmEntry = manifest.devEngines.packageManager.find((e) => e.name === 'pnpm')
          if (pnpmEntry) {
            const updated = updateVersionConstraint(pnpmEntry.version, resolution.manifest.version)
            if (updated !== pnpmEntry.version) {
              pnpmEntry.version = updated
              await writeProjectManifest(manifest)
            }
          }
        } else if (manifest.devEngines.packageManager.name === 'pnpm') {
          const updated = updateVersionConstraint(manifest.devEngines.packageManager.version, resolution.manifest.version)
          if (updated !== manifest.devEngines.packageManager.version) {
            manifest.devEngines.packageManager.version = updated
            await writeProjectManifest(manifest)
          }
        }
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

  // Link bins to pnpmHomeDir so the updated pnpm is the active global binary
  await linkBins(path.join(baseDir, 'node_modules'), opts.pnpmHomeDir, { warn: globalWarn })

  if (alreadyExisted) {
    return `The ${bareSpecifier} version, v${resolution.manifest.version}, is already present on the system. It was activated by linking it from ${baseDir}.`
  }
  return undefined
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
