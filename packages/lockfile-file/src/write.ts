import { promises as fs } from 'fs'
import path from 'path'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import { Lockfile, ProjectSnapshot } from '@pnpm/lockfile-types'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import rimraf from '@zkochan/rimraf'
import yaml from 'js-yaml'
import equals from 'ramda/src/equals'
import isEmpty from 'ramda/src/isEmpty'
import writeFileAtomicCB from 'write-file-atomic'
import logger from './logger'
import { sortLockfileKeys } from './sortLockfileKeys'

async function writeFileAtomic (filename: string, data: string) {
  return new Promise<void>((resolve, reject) => writeFileAtomicCB(filename, data, {}, (err?: Error) => (err != null) ? reject(err) : resolve()))
}

const LOCKFILE_YAML_FORMAT = {
  blankLines: true,
  lineWidth: 1000,
  noCompatMode: true,
  noRefs: true,
  sortKeys: false,
}

export async function writeWantedLockfile (
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

async function writeLockfile (
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
  let normalizedLockfile = normalizeLockfile(lockfile, forceSharedFormat)
  normalizedLockfile = sortLockfileKeys(normalizedLockfile)
  return yaml.dump(normalizedLockfile, LOCKFILE_YAML_FORMAT)
}

function isEmptyLockfile (lockfile: Lockfile) {
  return Object.values(lockfile.importers).every((importer) => isEmpty(importer.specifiers ?? {}) && isEmpty(importer.dependencies ?? {}))
}

export type LockfileFile = Omit<Lockfile, 'importers'> & Partial<ProjectSnapshot> & Partial<Pick<Lockfile, 'importers'>>

export function normalizeLockfile (lockfile: Lockfile, forceSharedFormat: boolean) {
  let lockfileToSave!: LockfileFile
  if (!forceSharedFormat && equals(Object.keys(lockfile.importers), ['.'])) {
    lockfileToSave = {
      ...lockfile,
      ...lockfile.importers['.'],
    }
    delete lockfileToSave.importers
    for (const depType of DEPENDENCIES_FIELDS) {
      if (isEmpty(lockfileToSave[depType])) {
        delete lockfileToSave[depType]
      }
    }
    if (isEmpty(lockfileToSave.packages) || (lockfileToSave.packages == null)) {
      delete lockfileToSave.packages
    }
  } else {
    lockfileToSave = {
      ...lockfile,
      importers: Object.keys(lockfile.importers).reduce((acc, alias) => {
        const importer = lockfile.importers[alias]
        const normalizedImporter = {
          specifiers: importer.specifiers ?? {},
        }
        if (importer.dependenciesMeta != null && !isEmpty(importer.dependenciesMeta)) {
          normalizedImporter['dependenciesMeta'] = importer.dependenciesMeta
        }
        for (const depType of DEPENDENCIES_FIELDS) {
          if (!isEmpty(importer[depType] ?? {})) {
            normalizedImporter[depType] = importer[depType]
          }
        }
        acc[alias] = normalizedImporter
        return acc
      }, {}),
    }
    if (isEmpty(lockfileToSave.packages) || (lockfileToSave.packages == null)) {
      delete lockfileToSave.packages
    }
  }
  if ((lockfileToSave.overrides != null) && isEmpty(lockfileToSave.overrides)) {
    delete lockfileToSave.overrides
  }
  if (lockfileToSave.neverBuiltDependencies != null) {
    if (isEmpty(lockfileToSave.neverBuiltDependencies)) {
      delete lockfileToSave.neverBuiltDependencies
    } else {
      lockfileToSave.neverBuiltDependencies = lockfileToSave.neverBuiltDependencies.sort()
    }
  }
  if (!lockfileToSave.packageExtensionsChecksum) {
    delete lockfileToSave.packageExtensionsChecksum
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
