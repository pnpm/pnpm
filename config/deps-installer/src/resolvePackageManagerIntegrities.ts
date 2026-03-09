import fs from 'fs'
import path from 'path'
import { install } from '@pnpm/core'
import { convertToLockfileFile, readWantedLockfile } from '@pnpm/lockfile.fs'
import type { StoreController } from '@pnpm/package-store'
import type { Registries } from '@pnpm/types'
import { readConfigLockfile, writeConfigLockfile, createConfigLockfile } from './configLockfile.js'
import { fastPathTemp as pathTemp } from 'path-temp'
import { sync as rimraf } from '@zkochan/rimraf'

export interface ResolvePackageManagerIntegritiesOpts {
  registries: Registries
  rootDir: string
  storeController: StoreController
  storeDir: string
}

/**
 * Resolves integrity checksums for `pnpm`, `@pnpm/exe`, and their dependencies
 * by calling @pnpm/core's install with lockfileOnly in a temp directory.
 * Writes the results to the `packageManagerDependencies` section of pnpm-config-lock.yaml.
 */
export async function resolvePackageManagerIntegrities (
  pnpmVersion: string,
  opts: ResolvePackageManagerIntegritiesOpts
): Promise<void> {
  const configLockfile = (await readConfigLockfile(opts.rootDir)) ?? createConfigLockfile()

  // Check if already resolved for this version
  const pmDeps = configLockfile.importers['.'].packageManagerDependencies
  if (pmDeps != null) {
    const hasVersion = Object.values(pmDeps).some((dep) => dep.version === pnpmVersion)
    if (hasVersion) return
  }

  const tempDir = pathTemp(path.join(opts.rootDir, 'node_modules', '.pnpm-tmp'))
  fs.mkdirSync(tempDir, { recursive: true })
  fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify({
    dependencies: {
      'pnpm': pnpmVersion,
      '@pnpm/exe': pnpmVersion,
    },
  }))

  try {
    await install(
      {
        dependencies: {
          'pnpm': pnpmVersion,
          '@pnpm/exe': pnpmVersion,
        },
      },
      {
        dir: tempDir,
        lockfileDir: tempDir,
        lockfileOnly: true,
        strictPeerDependencies: false,
        storeController: opts.storeController,
        storeDir: opts.storeDir,
        registries: opts.registries,
      }
    )

    const lockfile = await readWantedLockfile(tempDir, { ignoreIncompatible: true })
    if (lockfile?.packages) {
      const lockfileFile = convertToLockfileFile(lockfile)
      const packageManagerDependencies: Record<string, { specifier: string, version: string }> = {}
      for (const pkgName of ['pnpm', '@pnpm/exe']) {
        packageManagerDependencies[pkgName] = {
          specifier: pnpmVersion,
          version: pnpmVersion,
        }
      }
      for (const [depPath, pkgInfo] of Object.entries(lockfileFile.packages ?? {})) {
        configLockfile.packages[depPath] = pkgInfo
      }
      for (const [depPath, snapshotInfo] of Object.entries(lockfileFile.snapshots ?? {})) {
        configLockfile.snapshots[depPath] = snapshotInfo
      }
      configLockfile.importers['.'].packageManagerDependencies = packageManagerDependencies
      await writeConfigLockfile(opts.rootDir, configLockfile)
    }
  } finally {
    try {
      rimraf(tempDir)
    } catch {}
  }
}
