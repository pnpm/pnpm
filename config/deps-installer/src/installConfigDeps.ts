import fs from 'fs'
import path from 'path'
import { calcLeafGlobalVirtualStorePath } from '@pnpm/calc-dep-state'
import getNpmTarballUrl from 'get-npm-tarball-url'
import { installingConfigDepsLogger } from '@pnpm/core-loggers'
import { readModulesDir } from '@pnpm/read-modules-dir'
import rimraf from '@zkochan/rimraf'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import type { StoreController } from '@pnpm/package-store'
import type { ConfigDependencies, Registries } from '@pnpm/types'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import symlinkDir from 'symlink-dir'
import type { ConfigLockfile } from './configLockfile.js'
import { readConfigLockfile } from './configLockfile.js'
import { migrateConfigDepsToLockfile } from './migrateConfigDeps.js'

export interface InstallConfigDepsOpts {
  registries: Registries
  rootDir: string
  store: StoreController
  storeDir: string
}

interface NormalizedConfigDep {
  version: string
  resolution: {
    integrity: string
    tarball: string
  }
}

/**
 * Install config dependencies using the config lockfile.
 * Accepts either a ConfigLockfile directly (from resolveConfigDeps) or
 * ConfigDependencies from the workspace manifest (legacy/migration).
 */
export async function installConfigDeps (
  configDepsOrLockfile: ConfigDependencies | ConfigLockfile,
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
  configDepsOrLockfile: ConfigDependencies | ConfigLockfile,
  opts: InstallConfigDepsOpts
): Promise<Record<string, NormalizedConfigDep>> {
  // If it's a ConfigLockfile object (has lockfileVersion), use it directly
  if (isConfigLockfile(configDepsOrLockfile)) {
    return normalizeFromLockfile(configDepsOrLockfile, opts.registries)
  }

  // It's ConfigDependencies from workspace manifest.
  // Try to read the config lockfile first.
  const configLockfile = await readConfigLockfile(opts.rootDir)
  if (configLockfile) {
    return normalizeFromLockfile(configLockfile, opts.registries)
  }

  // No config lockfile yet — migrate from old inline integrity format
  return migrateConfigDepsToLockfile(configDepsOrLockfile, opts)
}

function isConfigLockfile (obj: ConfigDependencies | ConfigLockfile): obj is ConfigLockfile {
  return 'lockfileVersion' in obj
}

function normalizeFromLockfile (
  lockfile: ConfigLockfile,
  registries: Registries
): Record<string, NormalizedConfigDep> {
  const deps: Record<string, NormalizedConfigDep> = {}
  const configDeps = lockfile.importers['.'].configDependencies
  for (const [pkgName, { version }] of Object.entries(configDeps)) {
    const pkgKey = `${pkgName}@${version}`
    const pkgInfo = lockfile.packages[pkgKey]
    if (!pkgInfo) continue
    const registry = pickRegistryForPackage(registries, pkgName)
    deps[pkgName] = {
      version,
      resolution: {
        integrity: pkgInfo.resolution.integrity,
        tarball: pkgInfo.resolution.tarball ?? getNpmTarballUrl(pkgName, version, { registry }),
      },
    }
  }
  return deps
}
