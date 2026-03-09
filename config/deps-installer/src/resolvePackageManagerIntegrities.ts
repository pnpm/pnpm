import fs from 'fs'
import path from 'path'
import { runPnpmCli } from '@pnpm/exec.pnpm-cli-runner'
import { readWantedLockfile } from '@pnpm/lockfile.fs'
import { readConfigLockfile, writeConfigLockfile, createConfigLockfile } from './configLockfile.js'
import { fastPathTemp as pathTemp } from 'path-temp'
import { sync as rimraf } from '@zkochan/rimraf'

export interface ResolvePackageManagerIntegritiesOpts {
  rootDir: string
}

/**
 * Resolves integrity checksums for `pnpm`, `@pnpm/exe`, and their dependencies
 * by running `pnpm install --lockfile-only` in a temp directory.
 * Writes the results to the `packageManager` section of pnpm-config-lock.yaml.
 */
export async function resolvePackageManagerIntegrities (
  pnpmVersion: string,
  opts: ResolvePackageManagerIntegritiesOpts
): Promise<void> {
  const configLockfile = (await readConfigLockfile(opts.rootDir)) ?? createConfigLockfile()

  // Check if already resolved for this version
  if (configLockfile.packageManager != null) {
    const hasVersion = Object.keys(configLockfile.packageManager).some((key) => key.includes(`@${pnpmVersion}`))
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
    runPnpmCli([
      'install',
      '--lockfile-only',
      '--no-strict-peer-dependencies',
      '--loglevel=error',
    ], { cwd: tempDir })

    const lockfile = await readWantedLockfile(tempDir, { ignoreIncompatible: true })
    if (lockfile?.packages) {
      const packageManager: Record<string, { resolution: { integrity: string } }> = {}
      for (const [depPath, pkgInfo] of Object.entries(lockfile.packages)) {
        const integrity = 'integrity' in pkgInfo.resolution ? pkgInfo.resolution.integrity : undefined
        if (integrity) {
          packageManager[depPath] = {
            resolution: { integrity },
          }
        }
      }
      configLockfile.packageManager = packageManager
      await writeConfigLockfile(opts.rootDir, configLockfile)
    }
  } finally {
    try {
      rimraf(tempDir)
    } catch {}
  }
}
