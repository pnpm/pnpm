import { promises as fs } from 'fs'
import path from 'path'
import util from 'util'
import { ENV_LOCKFILE, LOCKFILE_VERSION } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import type { EnvLockfile } from '@pnpm/lockfile.types'
import { sortDirectKeys } from '@pnpm/object.key-sorting'
import yaml from 'js-yaml'
import stripBom from 'strip-bom'
import writeFileAtomic from 'write-file-atomic'
import { lockfileYamlDump } from './write.js'

export function createEnvLockfile (): EnvLockfile {
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

export async function readEnvLockfile (rootDir: string): Promise<EnvLockfile | null> {
  const lockfilePath = path.join(rootDir, ENV_LOCKFILE)
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
    throw new PnpmError('INVALID_ENV_LOCKFILE', `Invalid env lockfile at ${lockfilePath}: expected a YAML object`)
  }
  const lockfile = parsed as Record<string, unknown>
  if (typeof lockfile.lockfileVersion !== 'string') {
    throw new PnpmError('INVALID_ENV_LOCKFILE', `Invalid env lockfile at ${lockfilePath}: missing or non-string "lockfileVersion"`)
  }
  if (lockfile.importers == null || typeof lockfile.importers !== 'object') {
    throw new PnpmError('INVALID_ENV_LOCKFILE', `Invalid env lockfile at ${lockfilePath}: missing or invalid "importers"`)
  }
  if (lockfile.packages == null || typeof lockfile.packages !== 'object') {
    throw new PnpmError('INVALID_ENV_LOCKFILE', `Invalid env lockfile at ${lockfilePath}: missing or invalid "packages"`)
  }
  if (lockfile.snapshots == null || typeof lockfile.snapshots !== 'object') {
    throw new PnpmError('INVALID_ENV_LOCKFILE', `Invalid env lockfile at ${lockfilePath}: missing or invalid "snapshots"`)
  }
  const envLockfile = parsed as EnvLockfile
  if (!envLockfile.importers['.']) {
    envLockfile.importers['.'] = { configDependencies: {} }
  } else if (!envLockfile.importers['.'].configDependencies) {
    envLockfile.importers['.'].configDependencies = {}
  }
  return envLockfile
}

export async function writeEnvLockfile (rootDir: string, lockfile: EnvLockfile): Promise<void> {
  const lockfilePath = path.join(rootDir, ENV_LOCKFILE)
  const sorted = sortEnvLockfile(lockfile)
  const yamlDoc = lockfileYamlDump(sorted)
  return writeFileAtomic(lockfilePath, yamlDoc)
}

function sortEnvLockfile (lockfile: EnvLockfile): EnvLockfile {
  const importer: EnvLockfile['importers']['.'] = {
    configDependencies: sortDirectKeys(lockfile.importers['.']?.configDependencies ?? {}),
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
