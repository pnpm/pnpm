import path from 'path'
import getNpmTarballUrl from 'get-npm-tarball-url'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import { readModulesDir } from '@pnpm/read-modules-dir'
import rimraf from '@zkochan/rimraf'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { type StoreController } from '@pnpm/package-store'
import { type InstallDepsOptions } from './installDeps'

export async function installConfigDeps (configDeps: Record<string, string>, opts: InstallDepsOptions & {
  store: {
    ctrl: StoreController
    dir: string
  }
}): Promise<void> {
  const configModulesDir = path.join(opts.workspaceDir ?? opts.rootProjectManifestDir, 'node_modules/.pnpm-config')
  const existingConfigDeps: string[] = await readModulesDir(configModulesDir) ?? []
  await Promise.all(existingConfigDeps.map(async (existingConfigDep) => {
    if (!configDeps[existingConfigDep]) {
      await rimraf(path.join(configModulesDir, existingConfigDep))
    }
  }))
  await Promise.all(Object.entries(configDeps).map(async ([pkgName, pkgSpec]) => {
    const configDepPath = path.join(configModulesDir, pkgName)
    const sepIndex = pkgSpec.indexOf('+')
    const version = pkgSpec.substring(0, sepIndex)
    const integrity = pkgSpec.substring(sepIndex + 1)
    if (existingConfigDeps.includes(pkgName)) {
      const configDepPkgJson = await safeReadPackageJsonFromDir(configDepPath)
      if (configDepPkgJson == null || configDepPkgJson.name !== pkgName || configDepPkgJson.version !== version) {
        await rimraf(configDepPath)
      }
    }
    const registry = pickRegistryForPackage(opts.registries, pkgName)
    const fetchResult = await opts.store.ctrl.fetchPackage({
      force: true,
      lockfileDir: opts.lockfileDir ?? opts.dir,
      pkg: {
        id: `${pkgName}@${version}`,
        resolution: {
          tarball: getNpmTarballUrl(pkgName, version, { registry }),
          integrity,
        },
      },
    })
    await opts.store.ctrl.importPackage(configDepPath, {
      force: true,
      requiresBuild: false,
      filesResponse: (await fetchResult.fetching()).files,
    })
  }))
}
