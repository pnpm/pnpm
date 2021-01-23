import logger from './logger'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import { Lockfile, ProjectSnapshot } from '@pnpm/lockfile-types'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import rimraf = require('@zkochan/rimraf')
import yaml = require('js-yaml')
import fs = require('mz/fs')
import path = require('path')
import R = require('ramda')
import writeFileAtomicCB = require('write-file-atomic')

function writeFileAtomic (filename: string, data: string) {
  return new Promise<void>((resolve, reject) => writeFileAtomicCB(filename, data, {}, (err?: Error) => err ? reject(err) : resolve()))
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
    forceSharedFormat?: boolean
  }
) {
  return writeLockfile(WANTED_LOCKFILE, pkgPath, wantedLockfile, opts)
}

export async function writeCurrentLockfile (
  virtualStoreDir: string,
  currentLockfile: Lockfile,
  opts?: {
    forceSharedFormat?: boolean
  }
) {
  await fs.mkdir(virtualStoreDir, { recursive: true })
  return writeLockfile('lock.yaml', virtualStoreDir, currentLockfile, opts)
}

function writeLockfile (
  lockfileFilename: string,
  pkgPath: string,
  wantedLockfile: Lockfile,
  opts?: {
    forceSharedFormat?: boolean
  }
) {
  const lockfilePath = path.join(pkgPath, lockfileFilename)

  // empty lockfile is not saved
  if (isEmptyLockfile(wantedLockfile)) {
    return rimraf(lockfilePath)
  }

  const yamlDoc = yamlStringify(wantedLockfile, opts?.forceSharedFormat === true)

  return writeFileAtomic(lockfilePath, yamlDoc)
}

function yamlStringify (lockfile: Lockfile, forceSharedFormat: boolean) {
  const normalizedLockfile = normalizeLockfile(lockfile, forceSharedFormat)
  return yaml.dump(normalizedLockfile, LOCKFILE_YAML_FORMAT)
}

function isEmptyLockfile (lockfile: Lockfile) {
  return R.values(lockfile.importers).every((importer) => R.isEmpty(importer.specifiers ?? {}) && R.isEmpty(importer.dependencies ?? {}))
}

type LockfileFile = Omit<Lockfile, 'importers'> & Partial<ProjectSnapshot> & Partial<Pick<Lockfile, 'importers'>>

export function normalizeLockfile (lockfile: Lockfile, forceSharedFormat: boolean) {
  let lockfileToSave!: LockfileFile
  if (!forceSharedFormat && R.equals(R.keys(lockfile.importers), ['.'])) {
    lockfileToSave = {
      ...lockfile,
      ...lockfile.importers['.'],
    }
    delete lockfileToSave.importers
    for (const depType of DEPENDENCIES_FIELDS) {
      if (R.isEmpty(lockfileToSave[depType])) {
        delete lockfileToSave[depType]
      }
    }
    if (R.isEmpty(lockfileToSave.packages) || !lockfileToSave.packages) {
      delete lockfileToSave.packages
    }
  } else {
    lockfileToSave = {
      ...lockfile,
      importers: R.keys(lockfile.importers).reduce((acc, alias) => {
        const importer = lockfile.importers[alias]
        const normalizedImporter = {
          specifiers: importer.specifiers ?? {},
        }
        for (const depType of DEPENDENCIES_FIELDS) {
          if (!R.isEmpty(importer[depType] ?? {})) {
            normalizedImporter[depType] = importer[depType]
          }
        }
        acc[alias] = normalizedImporter
        return acc
      }, {}),
    }
    if (R.isEmpty(lockfileToSave.packages) || !lockfileToSave.packages) {
      delete lockfileToSave.packages
    }
  }
  if (lockfileToSave.overrides && R.isEmpty(lockfileToSave.overrides)) {
    delete lockfileToSave.overrides
  }
  if (lockfileToSave.neverBuiltDependencies) {
    if (R.isEmpty(lockfileToSave.neverBuiltDependencies)) {
      delete lockfileToSave.neverBuiltDependencies
    } else {
      lockfileToSave.neverBuiltDependencies = lockfileToSave.neverBuiltDependencies.sort()
    }
  }
  return lockfileToSave
}

export default async function writeLockfiles (
  opts: {
    forceSharedFormat?: boolean
    wantedLockfile: Lockfile
    wantedLockfileDir: string
    currentLockfile: Lockfile
    currentLockfileDir: string
  }
) {
  const wantedLockfilePath = path.join(opts.wantedLockfileDir, WANTED_LOCKFILE)
  const currentLockfilePath = path.join(opts.currentLockfileDir, 'lock.yaml')

  // empty lockfile is not saved
  if (isEmptyLockfile(opts.wantedLockfile)) {
    await Promise.all([
      rimraf(wantedLockfilePath),
      rimraf(currentLockfilePath),
    ])
    return
  }

  const forceSharedFormat = opts?.forceSharedFormat === true
  const yamlDoc = yamlStringify(opts.wantedLockfile, forceSharedFormat)

  // in most cases the `pnpm-lock.yaml` and `node_modules/.pnpm-lock.yaml` are equal
  // in those cases the YAML document can be stringified only once for both files
  // which is more efficient
  if (opts.wantedLockfile === opts.currentLockfile) {
    await Promise.all([
      writeFileAtomic(wantedLockfilePath, yamlDoc),
      (async () => {
        await fs.mkdir(path.dirname(currentLockfilePath), { recursive: true })
        await writeFileAtomic(currentLockfilePath, yamlDoc)
      })(),
    ])
    return
  }

  logger.debug({
    message: `\`${WANTED_LOCKFILE}\` differs from \`${path.relative(opts.wantedLockfileDir, currentLockfilePath)}\``,
    prefix: opts.wantedLockfileDir,
  })

  const currentYamlDoc = yamlStringify(opts.currentLockfile, forceSharedFormat)

  await Promise.all([
    writeFileAtomic(wantedLockfilePath, yamlDoc),
    (async () => {
      await fs.mkdir(path.dirname(currentLockfilePath), { recursive: true })
      await writeFileAtomic(currentLockfilePath, currentYamlDoc)
    })(),
  ])
}
