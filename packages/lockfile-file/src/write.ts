import { WANTED_LOCKFILE } from '@pnpm/constants'
import { Lockfile } from '@pnpm/lockfile-types'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import rimraf = require('@zkochan/rimraf')
import yaml = require('js-yaml')
import makeDir = require('make-dir')
import path = require('path')
import R = require('ramda')
import writeFileAtomicCB = require('write-file-atomic')
import logger from './logger'

function writeFileAtomic (filename: string, data: string) {
  return new Promise((resolve, reject) => writeFileAtomicCB(filename, data, {}, (err: Error) => err ? reject(err) : resolve()))
}

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
  virtualStoreDir: string,
  currentLockfile: Lockfile,
  opts?: {
    forceSharedFormat?: boolean,
  },
) {
  await makeDir(virtualStoreDir)
  return writeLockfile('lock.yaml', virtualStoreDir, currentLockfile, opts)
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
  opts: {
    forceSharedFormat?: boolean,
    wantedLockfile: Lockfile,
    wantedLockfileDir: string,
    currentLockfile: Lockfile,
    currentLockfileDir: string,
  },
) {
  const wantedLockfilePath = path.join(opts.wantedLockfileDir, WANTED_LOCKFILE)
  const currentLockfilePath = path.join(opts.currentLockfileDir, 'lock.yaml')

  // empty lockfile is not saved
  if (isEmptyLockfile(opts.wantedLockfile)) {
    return Promise.all([
      rimraf(wantedLockfilePath),
      rimraf(currentLockfilePath),
    ])
  }

  const forceSharedFormat = opts && opts.forceSharedFormat === true || false
  const yamlDoc = yaml.safeDump(normalizeLockfile(opts.wantedLockfile, forceSharedFormat), LOCKFILE_YAML_FORMAT)

  // in most cases the `pnpm-lock.yaml` and `node_modules/.pnpm-lock.yaml` are equal
  // in those cases the YAML document can be stringified only once for both files
  // which is more efficient
  if (opts.wantedLockfile === opts.currentLockfile) {
    return Promise.all([
      writeFileAtomic(wantedLockfilePath, yamlDoc),
      (async () => {
        await makeDir(path.dirname(currentLockfilePath))
        await writeFileAtomic(currentLockfilePath, yamlDoc)
      })(),
    ])
  }

  logger.debug({
    message: `\`${WANTED_LOCKFILE}\` differs from \`${path.relative(opts.wantedLockfileDir, currentLockfilePath)}\``,
    prefix: opts.wantedLockfileDir,
  })

  const currentYamlDoc = yaml.safeDump(normalizeLockfile(opts.currentLockfile, forceSharedFormat), LOCKFILE_YAML_FORMAT)

  return Promise.all([
    writeFileAtomic(wantedLockfilePath, yamlDoc),
    (async () => {
      await makeDir(path.dirname(currentLockfilePath))
      await writeFileAtomic(currentLockfilePath, currentYamlDoc)
    })(),
  ])
}
