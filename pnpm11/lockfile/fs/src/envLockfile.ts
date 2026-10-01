import path from 'node:path'

import { LOCKFILE_VERSION, WANTED_LOCKFILE } from '@pnpm/constants'
import type { EnvLockfile } from '@pnpm/lockfile.types'
import yaml from 'js-yaml'

import { sortLockfileKeys } from './sortLockfileKeys.js'
import { lockfileYamlDump, writeWantedLockfileAtomic } from './write.js'
import {
  extractMainDocument,
  readLockfileToString,
  streamReadFirstYamlDocument,
  YAML_DOCUMENT_SEPARATOR,
  YAML_DOCUMENT_START,
} from './yamlDocuments.js'

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
  if (!hasEnvLockfileShape(parsed as Record<string, unknown>)) {
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

function hasEnvLockfileShape (lockfile: Record<string, unknown>): boolean {
  return typeof lockfile.lockfileVersion === 'string' &&
    isObject(lockfile.importers) &&
    isObject(lockfile.packages) &&
    isObject(lockfile.snapshots)
}

function isObject (value: unknown): boolean {
  return value != null && typeof value === 'object'
}

/**
 * Replaces the env document that leads `pnpm-lock.yaml`, preserving the main
 * document after it. An unchanged document is not rewritten.
 */
export async function writeEnvLockfile (rootDir: string, lockfile: EnvLockfile): Promise<void> {
  const lockfilePath = path.join(rootDir, WANTED_LOCKFILE)
  const sorted = sortLockfileKeys(lockfile)
  const envYaml = lockfileYamlDump(sorted)

  const existing = await readLockfileToString(lockfilePath)
  const mainDoc = existing == null ? '' : extractMainDocument(existing)

  const combined = `${YAML_DOCUMENT_START}${envYaml}${YAML_DOCUMENT_SEPARATOR}${mainDoc}`
  if (existing === combined) return
  await writeWantedLockfileAtomic(lockfilePath, combined)
}
