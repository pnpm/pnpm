import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import util from 'node:util'

import { expect } from '@jest/globals'
import { assertStore } from '@pnpm/assert-store'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import type { Modules } from '@pnpm/installing.modules-yaml'
import type { LockfileFile } from '@pnpm/lockfile.types'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import yaml from 'js-yaml'
import { readYamlFileSync } from 'read-yaml-file'
import { writePackageSync } from 'write-package'

import { isExecutable } from './isExecutable.js'

const require = createRequire(import.meta.url)

export { isExecutable, type Modules }

export interface Project {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any -- a required module can export any shape and tests read arbitrary members from it
  requireModule: (moduleName: string) => any
  dir: () => string
  has: (pkgName: string, modulesDir?: string) => void
  hasNot: (pkgName: string, modulesDir?: string) => void
  getStorePath: () => string
  resolve: (pkgName: string, version?: string, relativePath?: string) => string
  getPkgIndexFilePath: (pkgName: string, version: string) => string
  cafsHas: (pkgName: string, version: string) => void
  cafsHasNot: (pkgName: string, version: string) => void
  storeHas: (pkgName: string, version?: string) => string
  storeHasNot: (pkgName: string, version?: string) => void
  isExecutable: (pathToExe: string) => void
  /**
   * TODO: Remove the `Required<T>` cast.
   *
   * https://github.com/microsoft/TypeScript/pull/32695 might help with this.
   */
  readCurrentLockfile: () => Required<LockfileFile>
  readModulesManifest: () => Modules | null
  /**
   * TODO: Remove the `Required<T>` cast.
   *
   * https://github.com/microsoft/TypeScript/pull/32695 might help with this.
   */
  readLockfile: (lockfileName?: string) => Required<LockfileFile>
  writePackageJson: (pkgJson: object) => void
}

export function assertProject (projectPath: string, encodedRegistryName?: string): Project {
  const modules = path.join(projectPath, 'node_modules')
  const getStoreInstance = createStoreInstanceGetter(modules, encodedRegistryName ?? `localhost+${REGISTRY_MOCK_PORT}`)

  const ok = (value: unknown): void => expect(value).toBeTruthy()
  const notOk = (value: unknown): void => expect(value).toBeFalsy()
  return {
    dir: () => projectPath,
    requireModule (pkgName: string) {
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
    getStorePath: () => getStoreInstance().storePath,
    resolve: (pkgName: string, version?: string, relativePath?: string) => getStoreInstance().resolve(pkgName, version, relativePath),
    getPkgIndexFilePath: (pkgName: string, version: string): string => getStoreInstance().getPkgIndexFilePath(pkgName, version),
    cafsHas (pkgName: string, version: string) {
      getStoreInstance().cafsHas(pkgName, version)
    },
    cafsHasNot (pkgName: string, version: string) {
      getStoreInstance().cafsHasNot(pkgName, version)
    },
    storeHas: (pkgName: string, version?: string) => getStoreInstance().resolve(pkgName, version),
    storeHasNot (pkgName: string, version?: string) {
      assertStoreHasNot(getStoreInstance, pkgName, version)
    },
    isExecutable (pathToExe: string) {
      isExecutable(ok, path.join(modules, pathToExe))
    },
    readCurrentLockfile: () => readCurrentLockfile(modules),
    readModulesManifest: () => readModulesManifest(modules),
    readLockfile: (lockfileName: string = WANTED_LOCKFILE) => readLockfile(path.join(projectPath, lockfileName)),
    writePackageJson (pkgJson: object) {
      writePackageSync(projectPath, pkgJson as any) // eslint-disable-line
    },
  }
}

interface StoreInstance {
  storePath: string
  getPkgIndexFilePath: (pkgName: string, version: string) => string
  cafsHas: (pkgName: string, version: string) => void
  cafsHasNot: (pkgName: string, version: string) => void
  storeHas: (pkgName: string, version?: string) => void
  storeHasNot: (pkgName: string, version?: string) => void
  resolve: (pkgName: string, version?: string, relativePath?: string) => string
}

function createStoreInstanceGetter (modules: string, encodedRegistryName: string): () => StoreInstance {
  let cachedStore: StoreInstance | undefined
  return () => {
    if (!cachedStore) {
      const modulesYaml = readModulesManifest(modules)
      if (modulesYaml == null) {
        throw new Error(`Cannot find module store. No .modules.yaml found at "${modules}"`)
      }
      const storePath = modulesYaml.storeDir
      cachedStore = {
        storePath,
        ...assertStore(storePath, encodedRegistryName),
      }
    }
    return cachedStore
  }
}

function assertStoreHasNot (getStoreInstance: () => StoreInstance, pkgName: string, version?: string): void {
  try {
    const store = getStoreInstance()
    store.storeHasNot(pkgName, version)
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && err.message.startsWith('Cannot find module store')) {
      return
    }
    throw err
  }
}

function getVirtualStoreDir (modules: string): string {
  const modulesYaml = readModulesManifest(modules)
  if (modulesYaml == null) {
    return path.join(modules, '.pnpm')
  }
  if (path.isAbsolute(modulesYaml.virtualStoreDir)) {
    return modulesYaml.virtualStoreDir
  }
  return path.join(modules, modulesYaml.virtualStoreDir)
}

function readCurrentLockfile (modules: string): Required<LockfileFile> {
  try {
    return readYamlFileSync(path.join(getVirtualStoreDir(modules), 'lock.yaml'))
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return null!
    throw err
  }
}

function readLockfile (lockfilePath: string): Required<LockfileFile> {
  try {
    const raw = fs.readFileSync(lockfilePath, 'utf8')
    // Skip the env lockfile document if present (first document in combined format).
    // Cannot import from @pnpm/lockfile.fs here due to circular dependency.
    let content = raw
    if (raw.startsWith('---\n')) {
      const sep = raw.indexOf('\n---\n')
      content = sep !== -1 ? raw.slice(sep + '\n---\n'.length) : ''
    }
    if (!content.trim()) return null!
    return yaml.load(content) as Required<LockfileFile>
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return null!
    throw err
  }
}

function readModulesManifest (modulesDir: string): Modules {
  try {
    return readYamlFileSync<Modules>(path.join(modulesDir, '.modules.yaml'))
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') return null!
    throw err
  }
}
