import { promises as fs } from 'fs'
import path from 'path'
import {
  LOCKFILE_VERSION,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import { Lockfile } from '@pnpm/lockfile-types'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import yaml from 'js-yaml'
import stripBom from 'strip-bom'
import { LockfileBreakingChangeError } from './errors'
import { autofixMergeConflicts, isDiff } from './gitMergeFile'
import logger from './logger'

export async function readCurrentLockfile (
  virtualStoreDir: string,
  opts: {
    wantedVersion?: number
    ignoreIncompatible: boolean
  }
): Promise<Lockfile | null> {
  const lockfilePath = path.join(virtualStoreDir, 'lock.yaml')
  return (await _read(lockfilePath, virtualStoreDir, opts)).lockfile
}

export async function readWantedLockfileAndAutofixConflicts (
  pkgPath: string,
  opts: {
    wantedVersion?: number
    ignoreIncompatible: boolean
  }
): Promise<{
    lockfile: Lockfile | null
    hadConflicts: boolean
  }> {
  const lockfilePath = path.join(pkgPath, WANTED_LOCKFILE)
  return _read(lockfilePath, pkgPath, { ...opts, autofixMergeConflicts: true })
}

export async function readWantedLockfile (
  pkgPath: string,
  opts: {
    wantedVersion?: number
    ignoreIncompatible: boolean
  }
): Promise<Lockfile | null> {
  const lockfilePath = path.join(pkgPath, WANTED_LOCKFILE)
  return (await _read(lockfilePath, pkgPath, opts)).lockfile
}

async function _read (
  lockfilePath: string,
  prefix: string,
  opts: {
    autofixMergeConflicts?: boolean
    wantedVersion?: number
    ignoreIncompatible: boolean
  }
): Promise<{
    lockfile: Lockfile | null
    hadConflicts: boolean
  }> {
  let lockfileRawContent
  try {
    lockfileRawContent = stripBom(await fs.readFile(lockfilePath, 'utf8'))
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
      throw err
    }
    return {
      lockfile: null,
      hadConflicts: false,
    }
  }
  let lockfile: Lockfile
  let hadConflicts!: boolean
  try {
    lockfile = yaml.load(lockfileRawContent) as Lockfile
    hadConflicts = false
  } catch (err) {
    if (!opts.autofixMergeConflicts || !isDiff(lockfileRawContent)) {
      throw new PnpmError('BROKEN_LOCKFILE', `The lockfile at "${lockfilePath}" is broken: ${err.message as string}`)
    }
    hadConflicts = true
    lockfile = autofixMergeConflicts(lockfileRawContent)
    logger.info({
      message: `Merge conflict detected in ${WANTED_LOCKFILE} and successfully merged`,
      prefix: path.dirname(lockfilePath),
    })
  }
  /* eslint-disable @typescript-eslint/dot-notation */
  if (typeof lockfile?.['specifiers'] !== 'undefined') {
    lockfile.importers = {
      '.': {
        specifiers: lockfile['specifiers'],
      },
    }
    delete lockfile['specifiers']
    for (const depType of DEPENDENCIES_FIELDS) {
      if (lockfile[depType]) {
        lockfile.importers['.'][depType] = lockfile[depType]
        delete lockfile[depType]
      }
    }
  }
  if (lockfile) {
    /* eslint-enable @typescript-eslint/dot-notation */
    if (typeof opts.wantedVersion !== 'number' || Math.floor(lockfile.lockfileVersion) === Math.floor(opts.wantedVersion)) {
      if (typeof opts.wantedVersion === 'number' && lockfile.lockfileVersion > opts.wantedVersion) {
        logger.warn({
          message: `Your ${WANTED_LOCKFILE} was generated by a newer version of pnpm. ` +
            `It is a compatible version but it might get downgraded to version ${opts.wantedVersion}`,
          prefix,
        })
      }
      return { lockfile, hadConflicts }
    }
  }
  if (opts.ignoreIncompatible) {
    logger.warn({
      message: `Ignoring not compatible lockfile at ${lockfilePath}`,
      prefix,
    })
    return { lockfile: null, hadConflicts: false }
  }
  throw new LockfileBreakingChangeError(lockfilePath)
}

export function createLockfileObject (
  importerIds: string[],
  opts: {
    lockfileVersion: number
  }
) {
  const importers = importerIds.reduce((acc, importerId) => {
    acc[importerId] = {
      dependencies: {},
      specifiers: {},
    }
    return acc
  }, {})
  return {
    importers,
    lockfileVersion: opts.lockfileVersion || LOCKFILE_VERSION,
  }
}
