import fs from 'fs'
import path from 'path'
import util from 'util'
import { assertStore } from '@pnpm/assert-store'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { type LockfileFileV9 } from '@pnpm/lockfile-types'
import { type Modules } from '@pnpm/modules-yaml'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import { sync as readYamlFile } from 'read-yaml-file'
import writePkg from 'write-pkg'
import isExecutable from './isExecutable'

export { isExecutable, type Modules }

export interface Project {
  // eslint-disable-next-line
  requireModule: (moduleName: string) => any
  dir: () => string
  has: (pkgName: string, modulesDir?: string) => void
  hasNot: (pkgName: string, modulesDir?: string) => void
  getStorePath: () => string
  resolve: (pkgName: string, version?: string, relativePath?: string) => string
  getPkgIndexFilePath: (pkgName: string, version?: string) => string
  cafsHas: (pkgName: string, version?: string) => void
  cafsHasNot: (pkgName: string, version?: string) => void
  storeHas: (pkgName: string, version?: string) => string
  storeHasNot: (pkgName: string, version?: string) => void
  isExecutable: (pathToExe: string) => void
  /**
   * TODO: Remove the `Required<T>` cast.
   *
   * https://github.com/microsoft/TypeScript/pull/32695 might help with this.
   */
  readCurrentLockfile: () => Required<LockfileFileV9>
  readModulesManifest: () => Modules | null
  /**
   * TODO: Remove the `Required<T>` cast.
   *
   * https://github.com/microsoft/TypeScript/pull/32695 might help with this.
   */
  readLockfile: (lockfileName?: string) => Required<LockfileFileV9>
  writePackageJson: (pkgJson: object) => void
}

export function assertProject (projectPath: string, encodedRegistryName?: string): Project {
  const ern = encodedRegistryName ?? `localhost+${REGISTRY_MOCK_PORT}`
  const modules = path.join(projectPath, 'node_modules')

  interface StoreInstance {
    storePath: string
    getPkgIndexFilePath: (pkgName: string, version?: string) => string
    cafsHas: (pkgName: string, version?: string) => void
    cafsHasNot: (pkgName: string, version?: string) => void
    storeHas: (pkgName: string, version?: string) => void
    storeHasNot: (pkgName: string, version?: string) => void
    resolve: (pkgName: string, version?: string, relativePath?: string) => string
  }
  let cachedStore: StoreInstance
  function getStoreInstance (): StoreInstance {
    if (!cachedStore) {
      const modulesYaml = readModulesManifest(modules)
      if (modulesYaml == null) {
        throw new Error(`Cannot find module store. No .modules.yaml found at "${modules}"`)
      }
      const storePath = modulesYaml.storeDir
      cachedStore = {
        storePath,
        ...assertStore(storePath, ern),
      }
    }
    return cachedStore
  }
  function getVirtualStoreDir (): string {
    const modulesYaml = readModulesManifest(modules)
    if (modulesYaml == null) {
      return path.join(modules, '.pnpm')
    }
    if (path.isAbsolute(modulesYaml.virtualStoreDir)) {
      return modulesYaml.virtualStoreDir
    }
    return path.join(modules, modulesYaml.virtualStoreDir)
  }

  // eslint-disable-next-line
  const ok = (value: any) => expect(value).toBeTruthy()
  // eslint-disable-next-line
  const notOk = (value: any) => expect(value).toBeFalsy()
  return {
    dir: () => projectPath,
    requireModule (pkgName: string) {
      // eslint-disable-next-line
      return require(path.join(modules, pkgName))
    },
    has (pkgName: string, _modulesDir?: string) {
      const md = _modulesDir ? path.join(projectPath, _modulesDir) : modules
      ok(fs.existsSync(path.join(md, pkgName)))
    },
    hasNot (pkgName: string, _modulesDir?: string) {
      const md = _modulesDir ? path.join(projectPath, _modulesDir) : modules
      notOk(fs.existsSync(path.join(md, pkgName)))
    },
    getStorePath () {
      const store = getStoreInstance()
      return store.storePath
    },
    resolve (pkgName: string, version?: string, relativePath?: string) {
      const store = getStoreInstance()
      return store.resolve(pkgName, version, relativePath)
    },
    getPkgIndexFilePath (pkgName: string, version?: string): string {
      const store = getStoreInstance()
      return store.getPkgIndexFilePath(pkgName, version)
    },
    cafsHas (pkgName: string, version?: string) {
      const store = getStoreInstance()
      store.cafsHas(pkgName, version)
    },
    cafsHasNot (pkgName: string, version?: string) {
      const store = getStoreInstance()
      store.cafsHasNot(pkgName, version)
    },
    storeHas (pkgName: string, version?: string) {
      const store = getStoreInstance()
      return store.resolve(pkgName, version)
    },
    storeHasNot (pkgName: string, version?: string) {
      try {
        const store = getStoreInstance()
        store.storeHasNot(pkgName, version)
      } catch (err: unknown) {
        if (util.types.isNativeError(err) && err.message.startsWith('Cannot find module store')) {
          return
        }
        throw err
      }
    },
    isExecutable (pathToExe: string) {
      isExecutable(ok, path.join(modules, pathToExe))
    },
    readCurrentLockfile () {
      try {
        return readYamlFile(path.join(getVirtualStoreDir(), 'lock.yaml'))
      } catch (err: unknown) {
        if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return null!
        throw err
      }
    },
    readModulesManifest: () => readModulesManifest(modules),
    readLockfile (lockfileName: string = WANTED_LOCKFILE) {
      try {
        return readYamlFile(path.join(projectPath, lockfileName))
      } catch (err: unknown) {
        if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return null!
        throw err
      }
    },
    writePackageJson (pkgJson: object) {
      writePkg.sync(projectPath, pkgJson as any) // eslint-disable-line
    },
  }
}

function readModulesManifest (modulesDir: string): Modules {
  try {
    return readYamlFile<Modules>(path.join(modulesDir, '.modules.yaml'))
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return null!
    throw err
  }
}
