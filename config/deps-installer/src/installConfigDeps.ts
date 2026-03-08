import fs from 'fs'
import path from 'path'
import { installingConfigDepsLogger } from '@pnpm/core-loggers'
import { hashObjectWithoutSorting, hashObject } from '@pnpm/crypto.object-hasher'
import { readModulesDir } from '@pnpm/read-modules-dir'
import rimraf from '@zkochan/rimraf'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import type { StoreController } from '@pnpm/package-store'
import type { ConfigDependencies, Registries } from '@pnpm/types'
import symlinkDir from 'symlink-dir'
import { normalizeConfigDeps } from './normalizeConfigDeps.js'

export interface InstallConfigDepsOpts {
  registries: Registries
  rootDir: string
  store: StoreController
  storeDir: string
}

export async function installConfigDeps (configDeps: ConfigDependencies, opts: InstallConfigDepsOpts): Promise<void> {
  const globalVirtualStoreDir = path.join(opts.storeDir, 'links')
  const configModulesDir = path.join(opts.rootDir, 'node_modules/.pnpm-config')
  const existingConfigDeps: string[] = await readModulesDir(configModulesDir) ?? []
  await Promise.all(existingConfigDeps.map(async (existingConfigDep) => {
    if (!configDeps[existingConfigDep]) {
      await rimraf(path.join(configModulesDir, existingConfigDep))
    }
  }))

  const installedConfigDeps: Array<{ name: string, version: string }> = []
  const normalizedConfigDeps = normalizeConfigDeps(configDeps, {
    registries: opts.registries,
  })
  await Promise.all(Object.entries(normalizedConfigDeps).map(async ([pkgName, pkg]) => {
    const configDepPath = path.join(configModulesDir, pkgName)
    const existingPkgJson = existingConfigDeps.includes(pkgName)
      ? await safeReadPackageJsonFromDir(configDepPath)
      : null
    if (existingPkgJson != null && existingPkgJson.name === pkgName && existingPkgJson.version === pkg.version) {
      return
    }
    installingConfigDepsLogger.debug({ status: 'started' })
    const dirInGlobalVirtualStore = calcConfigDepGlobalDir(globalVirtualStoreDir, pkgName, pkg.version, pkg.resolution.integrity)
    const pkgDirInGlobalVirtualStore = path.join(dirInGlobalVirtualStore, 'node_modules', pkgName)
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

function calcConfigDepGlobalDir (globalVirtualStoreDir: string, pkgName: string, version: string, integrity: string): string {
  const fullPkgId = `${pkgName}@${version}:${integrity}`
  const depsHash = hashObject({ id: fullPkgId, deps: {} })
  const hexDigest = hashObjectWithoutSorting({ engine: null, deps: depsHash }, { encoding: 'hex' })
  const prefix = pkgName.startsWith('@') ? '' : '@/'
  return path.join(globalVirtualStoreDir, `${prefix}${pkgName}/${version}/${hexDigest}`)
}
