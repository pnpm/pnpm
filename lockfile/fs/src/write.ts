import { promises as fs } from 'fs'
import path from 'path'
import { type LockfileObject, type LockfileFile } from '@pnpm/lockfile.types'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import rimraf from '@zkochan/rimraf'
import yaml from 'js-yaml'
import { isEmpty } from 'ramda'
import writeFileAtomicCB from 'write-file-atomic'
import { lockfileLogger as logger } from './logger.js'
import { sortLockfileKeys } from './sortLockfileKeys.js'
import { getWantedLockfileName } from './lockfileName.js'
import { convertToLockfileFile, type RandomDependency } from './lockfileFormatConverters.js'

export type { RandomDependency }

async function writeFileAtomic (filename: string, data: string): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    writeFileAtomicCB(filename, data, {}, (err?: Error) => {
      if (err != null) {
        reject(err)
      } else {
        resolve()
      }
    })
  })
}

const LOCKFILE_YAML_FORMAT = {
  blankLines: true,
  lineWidth: -1, // This is setting line width to never wrap
  noCompatMode: true,
  noRefs: true,
  sortKeys: false,
}

export async function writeWantedLockfile (
  pkgPath: string,
  wantedLockfile: LockfileObject,
  opts?: {
    useGitBranchLockfile?: boolean
    mergeGitBranchLockfiles?: boolean
  }
): Promise<WriteLockfileResult> {
  const wantedLockfileName: string = await getWantedLockfileName(opts)
  return writeLockfile(wantedLockfileName, pkgPath, wantedLockfile)
}

export async function writeCurrentLockfile (
  virtualStoreDir: string,
  currentLockfile: LockfileObject
): Promise<WriteLockfileResult> {
  // empty lockfile is not saved
  if (isEmptyLockfile(currentLockfile)) {
    await rimraf(path.join(virtualStoreDir, 'lock.yaml'))
    return { randomDependency: undefined }
  }
  await fs.mkdir(virtualStoreDir, { recursive: true })
  return writeLockfile('lock.yaml', virtualStoreDir, currentLockfile)
}

export interface WriteLockfileResult {
  randomDependency: RandomDependency | undefined
}

async function writeLockfile (
  lockfileFilename: string,
  pkgPath: string,
  wantedLockfile: LockfileObject
): Promise<WriteLockfileResult> {
  const lockfilePath = path.join(pkgPath, lockfileFilename)

  const { lockfile: lockfileToStringify, randomDependency } = convertToLockfileFile(wantedLockfile)
  await writeLockfileFile(lockfilePath, lockfileToStringify)
  return { randomDependency }
}

export function writeLockfileFile (
  lockfilePath: string,
  wantedLockfile: LockfileFile
): Promise<void> {
  const yamlDoc = yamlStringify(wantedLockfile)
  return writeFileAtomic(lockfilePath, yamlDoc)
}

function yamlStringify (lockfile: LockfileFile) {
  const sortedLockfile = sortLockfileKeys(lockfile as LockfileFile)
  return yaml.dump(sortedLockfile, LOCKFILE_YAML_FORMAT)
}

export function isEmptyLockfile (lockfile: LockfileObject): boolean {
  return Object.values(lockfile.importers).every((importer) => isEmpty(importer.specifiers ?? {}) && isEmpty(importer.dependencies ?? {}))
}

export async function writeLockfiles (
  opts: {
    wantedLockfile: LockfileObject
    wantedLockfileDir: string
    currentLockfile: LockfileObject
    currentLockfileDir: string
    useGitBranchLockfile?: boolean
    mergeGitBranchLockfiles?: boolean
  }
): Promise<WriteLockfileResult> {
  const wantedLockfileName: string = await getWantedLockfileName(opts)
  const wantedLockfilePath = path.join(opts.wantedLockfileDir, wantedLockfileName)
  const currentLockfilePath = path.join(opts.currentLockfileDir, 'lock.yaml')

  const { lockfile: wantedLockfileToStringify, randomDependency } = convertToLockfileFile(opts.wantedLockfile)
  const yamlDoc = yamlStringify(wantedLockfileToStringify)

  // in most cases the `pnpm-lock.yaml` and `node_modules/.pnpm-lock.yaml` are equal
  // in those cases the YAML document can be stringified only once for both files
  // which is more efficient
  if (opts.wantedLockfile === opts.currentLockfile) {
    await Promise.all([
      writeFileAtomic(wantedLockfilePath, yamlDoc),
      (async () => {
        if (isEmptyLockfile(opts.wantedLockfile)) {
          await rimraf(currentLockfilePath)
        } else {
          await fs.mkdir(path.dirname(currentLockfilePath), { recursive: true })
          await writeFileAtomic(currentLockfilePath, yamlDoc)
        }
      })(),
    ])
    return { randomDependency }
  }

  logger.debug({
    message: `\`${WANTED_LOCKFILE}\` differs from \`${path.relative(opts.wantedLockfileDir, currentLockfilePath)}\``,
    prefix: opts.wantedLockfileDir,
  })

  const { lockfile: currentLockfileToStringify } = convertToLockfileFile(opts.currentLockfile)
  const currentYamlDoc = yamlStringify(currentLockfileToStringify)

  await Promise.all([
    writeFileAtomic(wantedLockfilePath, yamlDoc),
    (async () => {
      if (isEmptyLockfile(opts.wantedLockfile)) {
        await rimraf(currentLockfilePath)
      } else {
        await fs.mkdir(path.dirname(currentLockfilePath), { recursive: true })
        await writeFileAtomic(currentLockfilePath, currentYamlDoc)
      }
    })(),
  ])
  return { randomDependency }
}
