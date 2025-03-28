import path from 'path'
import getNpmTarballUrl from 'get-npm-tarball-url'
import { PnpmError } from '@pnpm/error'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import { readModulesDir } from '@pnpm/read-modules-dir'
import rimraf from '@zkochan/rimraf'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import { type StoreController } from '@pnpm/package-store'
import { type Registries } from '@pnpm/types'

export async function installConfigDeps (configDeps: Record<string, string>, opts: {
  registries: Registries
  rootDir: string
  store: StoreController
}): Promise<void> {
  const configModulesDir = path.join(opts.rootDir, 'node_modules/.pnpm-config')
  const existingConfigDeps: string[] = await readModulesDir(configModulesDir) ?? []
  await Promise.all(existingConfigDeps.map(async (existingConfigDep) => {
    if (!configDeps[existingConfigDep]) {
      await rimraf(path.join(configModulesDir, existingConfigDep))
    }
  }))
  await Promise.all(Object.entries(configDeps).map(async ([pkgName, pkgSpec]) => {
    const configDepPath = path.join(configModulesDir, pkgName)
    const sepIndex = pkgSpec.indexOf('+')
    if (sepIndex === -1) {
      throw new PnpmError('CONFIG_DEP_NO_INTEGRITY', `Your config dependency called "${pkgName}" at "pnpm.configDependencies" doesn't have an integrity checksum`, {
        hint: `All config dependencies should have their integrity checksum inlined in the version specifier. For example:

{
  "pnpm": {
    "configDependencies": {
      "my-config": "1.0.0+sha512-Xg0tn4HcfTijTwfDwYlvVCl43V6h4KyVVX2aEm4qdO/PC6L2YvzLHFdmxhoeSA3eslcE6+ZVXHgWwopXYLNq4Q=="
    },
  }
}`,
      })
    }
    const version = pkgSpec.substring(0, sepIndex)
    const integrity = pkgSpec.substring(sepIndex + 1)
    if (existingConfigDeps.includes(pkgName)) {
      const configDepPkgJson = await safeReadPackageJsonFromDir(configDepPath)
      if (configDepPkgJson == null || configDepPkgJson.name !== pkgName || configDepPkgJson.version !== version) {
        await rimraf(configDepPath)
      }
    }
    const registry = pickRegistryForPackage(opts.registries, pkgName)
    const { fetching } = await opts.store.fetchPackage({
      force: true,
      lockfileDir: opts.rootDir,
      pkg: {
        id: `${pkgName}@${version}`,
        resolution: {
          tarball: getNpmTarballUrl(pkgName, version, { registry }),
          integrity,
        },
      },
    })
    const { files: filesResponse } = await fetching()
    await opts.store.importPackage(configDepPath, {
      force: true,
      requiresBuild: false,
      filesResponse,
    })
  }))
}
