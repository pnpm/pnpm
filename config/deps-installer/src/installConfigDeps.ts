import path from 'path'
import { installingConfigDepsLogger } from '@pnpm/core-loggers'
import { readModulesDir } from '@pnpm/read-modules-dir'
import rimraf from '@zkochan/rimraf'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { type StoreController } from '@pnpm/package-store'
import { type ConfigDependencies, type Registries } from '@pnpm/types'
import { normalizeConfigDeps } from './normalizeConfigDeps.js'

export interface InstallConfigDepsOpts {
  registries: Registries
  rootDir: string
  store: StoreController
}

export async function installConfigDeps (configDeps: ConfigDependencies, opts: InstallConfigDepsOpts): Promise<void> {
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
    if (existingConfigDeps.includes(pkgName)) {
      const configDepPkgJson = await safeReadPackageJsonFromDir(configDepPath)
      if (configDepPkgJson == null || configDepPkgJson.name !== pkgName || configDepPkgJson.version !== pkg.version) {
        await rimraf(configDepPath)
      } else {
        return
      }
    }
    installingConfigDepsLogger.debug({ status: 'started' })

    const { fetching } = await opts.store.fetchPackage({
      force: true,
      lockfileDir: opts.rootDir,
      pkg: {
        id: `${pkgName}@${pkg.version}`,
        resolution: pkg.resolution,
      },
    })

    const { files: filesResponse } = await fetching()
    await opts.store.importPackage(configDepPath, {
      force: true,
      requiresBuild: false,
      filesResponse,
    })
    installedConfigDeps.push({
      name: pkgName,
      version: pkg.version,
    })
  }))
  if (installedConfigDeps.length) {
    installingConfigDepsLogger.debug({ status: 'done', deps: installedConfigDeps })
  }
}
