import { promises as fs } from 'fs'
import path from 'path'
import util from 'util'
import { CONFIG_LOCKFILE, LOCKFILE_VERSION } from '@pnpm/constants'
import type { LockfilePackageInfo, LockfilePackageSnapshot } from '@pnpm/lockfile.types'
import yaml from 'js-yaml'
import writeFileAtomicCB from 'write-file-atomic'
import stripBom from 'strip-bom'

export interface ConfigLockfileImporterDep {
  specifier: string
  version: string
}

export interface ConfigLockfile {
  lockfileVersion: string
  importers: {
    '.': {
      configDependencies: Record<string, ConfigLockfileImporterDep>
      packageManagerDependencies?: Record<string, ConfigLockfileImporterDep>
    }
  }
  packages: Record<string, LockfilePackageInfo>
  snapshots: Record<string, LockfilePackageSnapshot>
}

const YAML_FORMAT = {
  blankLines: true,
  lineWidth: -1,
  noCompatMode: true,
  noRefs: true,
  sortKeys: false,
}

export function createConfigLockfile (): ConfigLockfile {
  return {
    lockfileVersion: LOCKFILE_VERSION,
    importers: {
      '.': {
        configDependencies: {},
      },
    },
    packages: {},
    snapshots: {},
  }
}

export async function readConfigLockfile (rootDir: string): Promise<ConfigLockfile | null> {
  const lockfilePath = path.join(rootDir, CONFIG_LOCKFILE)
  let rawContent: string
  try {
    rawContent = stripBom(await fs.readFile(lockfilePath, 'utf8'))
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return null
    }
    throw err
  }
  const parsed = yaml.load(rawContent)
  if (parsed == null || typeof parsed !== 'object') {
    throw new Error(`Invalid config lockfile at ${lockfilePath}: expected a YAML object`)
  }
  const lockfile = parsed as Record<string, unknown>
  if (typeof lockfile.lockfileVersion !== 'string') {
    throw new Error(`Invalid config lockfile at ${lockfilePath}: missing or non-string "lockfileVersion"`)
  }
  if (lockfile.importers == null || typeof lockfile.importers !== 'object') {
    throw new Error(`Invalid config lockfile at ${lockfilePath}: missing or invalid "importers"`)
  }
  if (lockfile.packages == null || typeof lockfile.packages !== 'object') {
    throw new Error(`Invalid config lockfile at ${lockfilePath}: missing or invalid "packages"`)
  }
  if (lockfile.snapshots == null || typeof lockfile.snapshots !== 'object') {
    throw new Error(`Invalid config lockfile at ${lockfilePath}: missing or invalid "snapshots"`)
  }
  return parsed as ConfigLockfile
}

export async function writeConfigLockfile (rootDir: string, lockfile: ConfigLockfile): Promise<void> {
  const lockfilePath = path.join(rootDir, CONFIG_LOCKFILE)
  const sorted = sortConfigLockfile(lockfile)
  const yamlDoc = yaml.dump(sorted, YAML_FORMAT)
  return new Promise<void>((resolve, reject) => {
    writeFileAtomicCB(lockfilePath, yamlDoc, {}, (err?: Error) => {
      if (err != null) {
        reject(err)
      } else {
        resolve()
      }
    })
  })
}

function sortConfigLockfile (lockfile: ConfigLockfile): ConfigLockfile {
  const importer: ConfigLockfile['importers']['.'] = {
    configDependencies: sortKeys(lockfile.importers['.'].configDependencies),
  }
  if (lockfile.importers['.'].packageManagerDependencies && Object.keys(lockfile.importers['.'].packageManagerDependencies).length > 0) {
    importer.packageManagerDependencies = sortKeys(lockfile.importers['.'].packageManagerDependencies)
  }
  return {
    lockfileVersion: lockfile.lockfileVersion,
    importers: {
      '.': importer,
    },
    packages: sortKeys(lockfile.packages),
    snapshots: sortKeys(lockfile.snapshots),
  }
}

function sortKeys<T> (obj: Record<string, T>): Record<string, T> {
  const sorted: Record<string, T> = {}
  for (const key of Object.keys(obj).sort()) {
    sorted[key] = obj[key]
  }
  return sorted
}
