import '@total-typescript/ts-reset'
import path from 'node:path'

import { assertStore } from '@pnpm/assert-store'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { readModulesManifest } from '@pnpm/modules-yaml'

import exists from 'path-exists'
import writePkg from 'write-pkg'
import readYamlFile from 'read-yaml-file'
// import type { JsonObject } from 'type-fest'

import type { AssertedProject, RawLockfile } from '@pnpm/types'

import { isExecutable } from './isExecutable.js'

export { isExecutable }

export function assertProject(
  projectPath: string,
  encodedRegistryName?: string | undefined
): AssertedProject {
  const ern = encodedRegistryName ?? `localhost+${REGISTRY_MOCK_PORT}`

  const modules = path.join(projectPath, 'node_modules')

  let cachedStore: {
    storePath: string
    getPkgIndexFilePath: (pkgName: string, version?: string) => Promise<string>
    cafsHas: (pkgName: string, version?: string | undefined) => Promise<void>
    cafsHasNot: (pkgName: string, version?: string | undefined) => Promise<void>
    storeHas: (pkgName: string, version?: string | undefined) => Promise<void>
    storeHasNot: (
      pkgName: string,
      version?: string | undefined
    ) => Promise<void>
    resolve: (
      pkgName: string,
      version?: string | undefined,
      relativePath?: string | undefined
    ) => Promise<string>
  }

  async function getStoreInstance() {
    if (!cachedStore) {
      const modulesYaml = await readModulesManifest(modules)

      if (modulesYaml == null) {
        throw new Error(
          `Cannot find module store. No .modules.yaml found at "${modules}"`
        )
      }

      const storePath = modulesYaml.storeDir

      cachedStore = {
        storePath,
        ...assertStore(storePath, ern),
      }
    }

    return cachedStore
  }

  async function getVirtualStoreDir() {
    const modulesYaml = await readModulesManifest(modules)

    if (modulesYaml == null) {
      return path.join(modules, '.pnpm')
    }

    return modulesYaml.virtualStoreDir
  }

  // eslint-disable-next-line
  const ok = (value: any) => expect(value).toBeTruthy()

  // eslint-disable-next-line
  const notOk = (value: any) => expect(value).toBeFalsy()

  return {
    dir: () => projectPath,
    requireModule(pkgName: string) {
      // TODO: fix require
      // eslint-disable-next-line
      return require(path.join(modules, pkgName))
    },
    async has(pkgName: string, _modulesDir?: string | undefined) {
      const md = _modulesDir ? path.join(projectPath, _modulesDir) : modules

      ok(await exists(path.join(md, pkgName)))
    },
    async hasNot(pkgName: string, _modulesDir?: string | undefined) {
      const md = _modulesDir ? path.join(projectPath, _modulesDir) : modules

      notOk(await exists(path.join(md, pkgName)))
    },
    async getStorePath() {
      const store = await getStoreInstance()

      return store.storePath
    },
    async resolve(pkgName: string, version?: string | undefined, relativePath?: string | undefined) {
      const store = await getStoreInstance()
      return store.resolve(pkgName, version, relativePath)
    },
    async getPkgIndexFilePath(
      pkgName: string,
      version?: string | undefined
    ): Promise<string> {
      const store = await getStoreInstance()
      return store.getPkgIndexFilePath(pkgName, version)
    },
    async cafsHas(pkgName: string, version?: string | undefined) {
      const store = await getStoreInstance()
      return store.cafsHas(pkgName, version)
    },
    async cafsHasNot(pkgName: string, version?: string | undefined) {
      const store = await getStoreInstance()
      return store.cafsHasNot(pkgName, version)
    },
    async storeHas(pkgName: string, version?: string | undefined) {
      const store = await getStoreInstance()
      return store.resolve(pkgName, version)
    },
    async storeHasNot(pkgName: string, version?: string | undefined) {
      try {
        const store = await getStoreInstance()

        return store.storeHasNot(pkgName, version)
      } catch (err: unknown) {
        // @ts-ignore
        if (err.message.startsWith('Cannot find module store')) {
          return
        }
        throw err
      }
    },
    async isExecutable(pathToExe: string) {
      return isExecutable(ok, path.join(modules, pathToExe))
    },
    async readCurrentLockfile() {
      try {
        return await readYamlFile.default(
          path.join(await getVirtualStoreDir(), 'lock.yaml')
        )
      } catch (err: unknown) {
        // @ts-ignore
        if (err.code === 'ENOENT') return null!
        throw err
      }
    },
    readModulesManifest: async () => readModulesManifest(modules),
    async readLockfile(lockfileName: string = WANTED_LOCKFILE): Promise<Required<RawLockfile>> {
      try {
        return await readYamlFile.default(path.join(projectPath, lockfileName))
      } catch (err: unknown) {
        // @ts-ignore
        if (err.code === 'ENOENT') return null!
        throw err
      }
    },
    async writePackageJson(pkgJson) {
      // @ts-ignore
      return writePkg(projectPath, pkgJson)
    },
  }
}
