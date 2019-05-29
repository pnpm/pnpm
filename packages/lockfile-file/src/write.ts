import {
  CURRENT_LOCKFILE,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import { Lockfile } from '@pnpm/lockfile-types'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import yaml = require('js-yaml')
import makeDir = require('make-dir')
import path = require('path')
import R = require('ramda')
import rimraf = require('rimraf-then')
import { promisify } from 'util'
import writeFileAtomicCB = require('write-file-atomic')
import logger from './logger'

const writeFileAtomic = promisify(writeFileAtomicCB)

const LOCKFILE_YAML_FORMAT = {
  lineWidth: 1000,
  noCompatMode: true,
  noRefs: true,
  sortKeys: true,
}

export function writeWantedLockfile (
  pkgPath: string,
  wantedLockfile: Lockfile,
  opts?: {
    forceSharedFormat?: boolean,
  },
) {
  return writeLockfile(WANTED_LOCKFILE, pkgPath, wantedLockfile, opts)
}

export async function writeCurrentLockfile (
  pkgPath: string,
  currentLockfile: Lockfile,
  opts?: {
    forceSharedFormat?: boolean,
  },
) {
  await makeDir(path.join(pkgPath, 'node_modules'))
  return writeLockfile(CURRENT_LOCKFILE, pkgPath, currentLockfile, opts)
}

function writeLockfile (
  lockfileFilename: string,
  pkgPath: string,
  wantedLockfile: Lockfile,
  opts?: {
    forceSharedFormat?: boolean,
  },
) {
  const lockfilePath = path.join(pkgPath, lockfileFilename)

  // empty lockfile is not saved
  if (isEmptyLockfile(wantedLockfile)) {
    return rimraf(lockfilePath)
  }

  const yamlDoc = yaml.safeDump(normalizeLockfile(wantedLockfile, opts && opts.forceSharedFormat === true || false), LOCKFILE_YAML_FORMAT)

  return writeFileAtomic(lockfilePath, yamlDoc)
}

function isEmptyLockfile (lockfile: Lockfile) {
  return R.values(lockfile.importers).every((importer) => R.isEmpty(importer.specifiers || {}) && R.isEmpty(importer.dependencies || {}))
}

function normalizeLockfile (lockfile: Lockfile, forceSharedFormat: boolean) {
  if (forceSharedFormat === false && R.equals(R.keys(lockfile.importers), ['.'])) {
    const lockfileToSave = {
      ...lockfile,
      ...lockfile.importers['.'],
    }
    delete lockfileToSave.importers
    for (const depType of DEPENDENCIES_FIELDS) {
      if (R.isEmpty(lockfileToSave[depType])) {
        delete lockfileToSave[depType]
      }
    }
    if (R.isEmpty(lockfileToSave.packages)) {
      delete lockfileToSave.packages
    }
    return lockfileToSave
  } else {
    const lockfileToSave = {
      ...lockfile,
      importers: R.keys(lockfile.importers).reduce((acc, alias) => {
        const importer = lockfile.importers[alias]
        const normalizedImporter = {
          specifiers: importer.specifiers,
        }
        for (const depType of DEPENDENCIES_FIELDS) {
          if (!R.isEmpty(importer[depType] || {})) {
            normalizedImporter[depType] = importer[depType]
          }
        }
        acc[alias] = normalizedImporter
        return acc
      }, {}),
    }
    if (R.isEmpty(lockfileToSave.packages)) {
      delete lockfileToSave.packages
    }
    return lockfileToSave
  }
}

export default function writeLockfiles (
  pkgPath: string,
  wantedLockfile: Lockfile,
  currentLockfile: Lockfile,
  opts?: {
    forceSharedFormat?: boolean,
  },
) {
  const wantedLockfilePath = path.join(pkgPath, WANTED_LOCKFILE)
  const currentLockfilePath = path.join(pkgPath, CURRENT_LOCKFILE)

  // empty lockfile is not saved
  if (isEmptyLockfile(wantedLockfile)) {
    return Promise.all([
      rimraf(wantedLockfilePath),
      rimraf(currentLockfilePath),
    ])
  }

  const forceSharedFormat = opts && opts.forceSharedFormat === true || false
  const yamlDoc = yaml.safeDump(normalizeLockfile(wantedLockfile, forceSharedFormat), LOCKFILE_YAML_FORMAT)

  // in most cases the `pnpm-lock.yaml` and `node_modules/.pnpm-lock.yaml` are equal
  // in those cases the YAML document can be stringified only once for both files
  // which is more efficient
  if (wantedLockfile === currentLockfile) {
    return Promise.all([
      writeFileAtomic(wantedLockfilePath, yamlDoc),
      (async () => {
        await makeDir(path.dirname(currentLockfilePath))
        await writeFileAtomic(currentLockfilePath, yamlDoc)
      })(),
    ])
  }

  logger.debug({
    message: `\`${WANTED_LOCKFILE}\` differs from \`${CURRENT_LOCKFILE}\``,
    prefix: pkgPath,
  })

  const currentYamlDoc = yaml.safeDump(normalizeLockfile(currentLockfile, forceSharedFormat), LOCKFILE_YAML_FORMAT)

  return Promise.all([
    writeFileAtomic(wantedLockfilePath, yamlDoc),
    (async () => {
      await makeDir(path.dirname(currentLockfilePath))
      await writeFileAtomic(currentLockfilePath, currentYamlDoc)
    })(),
  ])
}
