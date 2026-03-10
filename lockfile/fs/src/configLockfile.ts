import { promises as fs } from 'fs'
import path from 'path'
import util from 'util'
import { CONFIG_LOCKFILE, LOCKFILE_VERSION } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import type { ConfigLockfile } from '@pnpm/lockfile.types'
import { sortDirectKeys } from '@pnpm/object.key-sorting'
import yaml from 'js-yaml'
import stripBom from 'strip-bom'
import writeFileAtomic from 'write-file-atomic'
import { lockfileYamlDump } from './write.js'

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
    throw new PnpmError('INVALID_CONFIG_LOCKFILE', `Invalid config lockfile at ${lockfilePath}: expected a YAML object`)
  }
  const lockfile = parsed as Record<string, unknown>
  if (typeof lockfile.lockfileVersion !== 'string') {
    throw new PnpmError('INVALID_CONFIG_LOCKFILE', `Invalid config lockfile at ${lockfilePath}: missing or non-string "lockfileVersion"`)
  }
  if (lockfile.importers == null || typeof lockfile.importers !== 'object') {
    throw new PnpmError('INVALID_CONFIG_LOCKFILE', `Invalid config lockfile at ${lockfilePath}: missing or invalid "importers"`)
  }
  if (lockfile.packages == null || typeof lockfile.packages !== 'object') {
    throw new PnpmError('INVALID_CONFIG_LOCKFILE', `Invalid config lockfile at ${lockfilePath}: missing or invalid "packages"`)
  }
  if (lockfile.snapshots == null || typeof lockfile.snapshots !== 'object') {
    throw new PnpmError('INVALID_CONFIG_LOCKFILE', `Invalid config lockfile at ${lockfilePath}: missing or invalid "snapshots"`)
  }
  return parsed as ConfigLockfile
}

export async function writeConfigLockfile (rootDir: string, lockfile: ConfigLockfile): Promise<void> {
  const lockfilePath = path.join(rootDir, CONFIG_LOCKFILE)
  const sorted = sortConfigLockfile(lockfile)
  const yamlDoc = lockfileYamlDump(sorted)
  return writeFileAtomic(lockfilePath, yamlDoc)
}

function sortConfigLockfile (lockfile: ConfigLockfile): ConfigLockfile {
  const importer: ConfigLockfile['importers']['.'] = {
    configDependencies: sortDirectKeys(lockfile.importers['.'].configDependencies),
  }
  if (lockfile.importers['.'].packageManagerDependencies && Object.keys(lockfile.importers['.'].packageManagerDependencies).length > 0) {
    importer.packageManagerDependencies = sortDirectKeys(lockfile.importers['.'].packageManagerDependencies)
  }
  return {
    lockfileVersion: lockfile.lockfileVersion,
    importers: {
      '.': importer,
    },
    packages: sortDirectKeys(lockfile.packages),
    snapshots: sortDirectKeys(lockfile.snapshots),
  }
}
