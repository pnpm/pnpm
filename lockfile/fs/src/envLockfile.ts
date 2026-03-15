import { promises as fs } from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import type { EnvLockfile } from '@pnpm/lockfile.types'
import { sortDirectKeys } from '@pnpm/object.key-sorting'
import yaml from 'js-yaml'
import writeFileAtomic from 'write-file-atomic'

import { lockfileYamlDump } from './write.js'
import { extractMainDocument, streamReadFirstYamlDocument } from './yamlDocuments.js'

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
  const lockfilePath = path.join(rootDir, WANTED_LOCKFILE)
  const rawContent = await streamReadFirstYamlDocument(lockfilePath)
  if (rawContent == null) {
    return null
  }
  const parsed = yaml.load(rawContent)
  if (parsed == null || typeof parsed !== 'object') {
    return null
  }
  const lockfile = parsed as Record<string, unknown>
  if (typeof lockfile.lockfileVersion !== 'string') {
    return null
  }
  if (lockfile.importers == null || typeof lockfile.importers !== 'object') {
    return null
  }
  if (lockfile.packages == null || typeof lockfile.packages !== 'object') {
    return null
  }
  if (lockfile.snapshots == null || typeof lockfile.snapshots !== 'object') {
    return null
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
  const lockfilePath = path.join(rootDir, WANTED_LOCKFILE)
  const sorted = sortEnvLockfile(lockfile)
  const envYaml = lockfileYamlDump(sorted)

  // Read existing main lockfile document to preserve it
  let mainDoc = ''
  try {
    const existing = await fs.readFile(lockfilePath, 'utf8')
    mainDoc = extractMainDocument(existing)
  } catch (err: unknown) {
    if (!(util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT')) {
      throw err
    }
  }

  const combined = `---\n${envYaml}\n---\n${mainDoc}`
  return writeFileAtomic(lockfilePath, combined)
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
