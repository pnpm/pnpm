import fs from 'node:fs'
import path from 'node:path'

import { calcLeafGlobalVirtualStorePath } from '@pnpm/calc-dep-state'
import { installingConfigDepsLogger } from '@pnpm/core-loggers'
import { PnpmError } from '@pnpm/error'
import { type EnvLockfile, readEnvLockfile } from '@pnpm/lockfile.fs'
import type { StoreController } from '@pnpm/package-store'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import { readModulesDir } from '@pnpm/read-modules-dir'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import type { ConfigDependencies, Registries } from '@pnpm/types'
import { rimraf } from '@zkochan/rimraf'
import getNpmTarballUrl from 'get-npm-tarball-url'
import symlinkDir from 'symlink-dir'

import { migrateConfigDepsToLockfile } from './migrateConfigDeps.js'
import type { NormalizedConfigDep } from './parseIntegrity.js'

export interface InstallConfigDepsOpts {
  registries: Registries
  rootDir: string
  store: StoreController
  storeDir: string
}

/**
 * Install config dependencies using the env lockfile.
 * Accepts either a EnvLockfile directly (from resolveConfigDeps) or
 * ConfigDependencies from the workspace manifest (legacy/migration).
 */
export async function installConfigDeps (
  configDepsOrLockfile: ConfigDependencies | EnvLockfile,
  opts: InstallConfigDepsOpts
): Promise<void> {
  const normalizedDeps = await normalizeForInstall(configDepsOrLockfile, opts)
  const globalVirtualStoreDir = path.join(opts.storeDir, 'links')

  const configModulesDir = path.join(opts.rootDir, 'node_modules/.pnpm-config')
  const existingConfigDeps: string[] = await readModulesDir(configModulesDir) ?? []
  await Promise.all(existingConfigDeps.map(async (existingConfigDep) => {
    if (!normalizedDeps[existingConfigDep]) {
      await rimraf(path.join(configModulesDir, existingConfigDep))
    }
  }))

  const installedConfigDeps: Array<{ name: string, version: string }> = []
  await Promise.all(Object.entries(normalizedDeps).map(async ([pkgName, pkg]) => {
    const configDepPath = path.join(configModulesDir, pkgName)
    const existingPkgJson = existingConfigDeps.includes(pkgName)
      ? await safeReadPackageJsonFromDir(configDepPath)
      : null
    if (existingPkgJson != null && existingPkgJson.name === pkgName && existingPkgJson.version === pkg.version) {
      return
    }
    installingConfigDepsLogger.debug({ status: 'started' })
    const fullPkgId = `${pkgName}@${pkg.version}:${pkg.resolution.integrity}`
    const relPath = calcLeafGlobalVirtualStorePath(fullPkgId, pkgName, pkg.version)
    const pkgDirInGlobalVirtualStore = path.join(globalVirtualStoreDir, relPath, 'node_modules', pkgName)
    if (!fs.existsSync(path.join(pkgDirInGlobalVirtualStore, 'package.json'))) {
      const { fetching } = await opts.store.fetchPackage({
        force: true,
        lockfileDir: opts.rootDir,
        pkg: {
          id: `${pkgName}@${pkg.version}`,
          resolution: pkg.resolution,
        },
      })
      const { files: filesResponse } = await fetching()
      await opts.store.importPackage(pkgDirInGlobalVirtualStore, {
        force: true,
        requiresBuild: false,
        filesResponse,
      })
    }
    if (existingConfigDeps.includes(pkgName)) {
      await rimraf(configDepPath)
    }
    await fs.promises.mkdir(path.dirname(configDepPath), { recursive: true })
    await symlinkDir(pkgDirInGlobalVirtualStore, configDepPath)
    installedConfigDeps.push({
      name: pkgName,
      version: pkg.version,
    })
  }))
  if (installedConfigDeps.length) {
    installingConfigDepsLogger.debug({ status: 'done', deps: installedConfigDeps })
  }
}

async function normalizeForInstall (
  configDepsOrLockfile: ConfigDependencies | EnvLockfile,
  opts: InstallConfigDepsOpts
): Promise<Record<string, NormalizedConfigDep>> {
  // If it's a EnvLockfile object (has lockfileVersion), use it directly
  if (isEnvLockfile(configDepsOrLockfile)) {
    return normalizeFromLockfile(configDepsOrLockfile, opts.registries)
  }

  // It's ConfigDependencies from workspace manifest.
  // Try to read the env lockfile first.
  const envLockfile = await readEnvLockfile(opts.rootDir)
  if (envLockfile) {
    return normalizeFromLockfile(envLockfile, opts.registries)
  }

  // No env lockfile yet — migrate from old inline integrity format
  return migrateConfigDepsToLockfile(configDepsOrLockfile, opts)
}

function isEnvLockfile (obj: ConfigDependencies | EnvLockfile): obj is EnvLockfile {
  return 'lockfileVersion' in obj &&
    'importers' in obj &&
    obj.importers != null &&
    typeof obj.importers === 'object' &&
    'packages' in obj &&
    obj.packages != null &&
    typeof obj.packages === 'object' &&
    'snapshots' in obj &&
    obj.snapshots != null &&
    typeof obj.snapshots === 'object'
}

function normalizeFromLockfile (
  lockfile: EnvLockfile,
  registries: Registries
): Record<string, NormalizedConfigDep> {
  const deps: Record<string, NormalizedConfigDep> = {}
  const configDeps = lockfile.importers['.']?.configDependencies ?? {}
  for (const [pkgName, { version }] of Object.entries(configDeps)) {
    const pkgKey = `${pkgName}@${version}`
    const pkgInfo = lockfile.packages[pkgKey]
    if (!pkgInfo) {
      throw new PnpmError(
        'ENV_LOCKFILE_CORRUPTED',
        `pnpm-lock.yaml is corrupted or incomplete: missing packages entry for "${pkgKey}" ` +
        'referenced from importers[\'.\'].configDependencies'
      )
    }
    const resolution = pkgInfo.resolution as { integrity?: string; tarball?: string }
    if (!resolution.integrity) {
      throw new PnpmError(
        'ENV_LOCKFILE_CORRUPTED',
        `pnpm-lock.yaml is corrupted or incomplete: missing integrity for "${pkgKey}"`
      )
    }
    const registry = pickRegistryForPackage(registries, pkgName)
    deps[pkgName] = {
      version,
      resolution: {
        integrity: resolution.integrity,
        tarball: resolution.tarball ?? getNpmTarballUrl(pkgName, version, { registry }),
      },
    }
  }
  return deps
}
