import path from 'path'
import { createCafsStore } from '@pnpm/create-cafs-store'
import { createFetchFromRegistry } from '@pnpm/fetch'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createTarballFetcher } from '@pnpm/tarball-fetcher'
import getNpmTarballUrl from 'get-npm-tarball-url'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import { getStorePath } from '@pnpm/store-path'
import { type InstallDepsOptions } from './installDeps'

export async function installConfigDeps (configDeps: Record<string, string>, opts: InstallDepsOptions): Promise<void> {
  const fetchFromRegistry = createFetchFromRegistry(opts)
  const getAuthHeader = createGetAuthHeaderByURI({ allSettings: opts.rawConfig, userSettings: opts.userConfig })
  const { remoteTarball } = createTarballFetcher(fetchFromRegistry, getAuthHeader, opts)
  const storeDir = await getStorePath({
    pkgRoot: opts.workspaceDir ?? opts.dir,
    storePath: opts.storeDir,
    pnpmHomeDir: opts.pnpmHomeDir,
  })
  const cafs = createCafsStore(storeDir, {
    packageImportMethod: opts.packageImportMethod,
  })
  const configModulesDir = path.join(opts.workspaceDir ?? opts.dir, 'node_modules/.pnpm-config')
  await Promise.all(Object.entries(configDeps).map(async ([pkgName, pkgSpec]) => {
    const sepIndex = pkgSpec.indexOf('+')
    const version = pkgSpec.substring(0, sepIndex)
    const integrity = pkgSpec.substring(sepIndex + 1)
    const registry = pickRegistryForPackage(opts.registries, pkgName)
    const filesIndexFile = cafs.getIndexFilePathInCafs(integrity, 'index')
    const fetchResult = await remoteTarball(cafs, {
      tarball: getNpmTarballUrl(pkgName, version, { registry }),
      integrity,
    }, { filesIndexFile, lockfileDir: opts.lockfileDir ?? opts.dir, pkg: { name: pkgName, version: version } })
    cafs.importPackage(path.join(configModulesDir, pkgName), {
      force: true,
      requiresBuild: false,
      filesResponse: {
        requiresBuild: false,
        resolvedFrom: 'remote',
        filesIndex: fetchResult.filesIndex,
      },
    })
  }))
}
