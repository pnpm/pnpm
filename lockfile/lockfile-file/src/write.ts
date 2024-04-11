import { promises as fs } from 'fs'
import path from 'path'
import { type Lockfile, type LockfileFile } from '@pnpm/lockfile-types'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import rimraf from '@zkochan/rimraf'
import yaml from 'js-yaml'
import isEmpty from 'ramda/src/isEmpty'
import writeFileAtomicCB from 'write-file-atomic'
import { lockfileLogger as logger } from './logger'
import { sortLockfileKeys } from './sortLockfileKeys'
import { getWantedLockfileName } from './lockfileName'
import { convertToLockfileFile } from './lockfileFormatConverters'

async function writeFileAtomic (filename: string, data: string): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    writeFileAtomicCB(filename, data, {}, (err?: Error) => {
      (err != null) ? reject(err) : resolve()
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
  wantedLockfile: Lockfile,
  opts?: {
    useGitBranchLockfile?: boolean
    mergeGitBranchLockfiles?: boolean
  }
): Promise<void> {
  const wantedLockfileName: string = await getWantedLockfileName(opts)
  return writeLockfile(wantedLockfileName, pkgPath, wantedLockfile)
}

export async function writeCurrentLockfile (
  virtualStoreDir: string,
  currentLockfile: Lockfile
): Promise<void> {
  // empty lockfile is not saved
  if (isEmptyLockfile(currentLockfile)) {
    await rimraf(path.join(virtualStoreDir, 'lock.yaml'))
    return
  }
  await fs.mkdir(virtualStoreDir, { recursive: true })
  return writeLockfile('lock.yaml', virtualStoreDir, currentLockfile)
}

async function writeLockfile (
  lockfileFilename: string,
  pkgPath: string,
  wantedLockfile: Lockfile
): Promise<void> {
  const lockfilePath = path.join(pkgPath, lockfileFilename)

  const lockfileToStringify = convertToLockfileFile(wantedLockfile, {
    forceSharedFormat: true,
  })

  const yamlDoc = yamlStringify(lockfileToStringify)

  return writeFileAtomic(lockfilePath, yamlDoc)
}

function yamlStringify (lockfile: LockfileFile) {
  const sortedLockfile = sortLockfileKeys(lockfile)
  return yaml.dump(sortedLockfile, LOCKFILE_YAML_FORMAT)
}

export function isEmptyLockfile (lockfile: Lockfile): boolean {
  return Object.values(lockfile.importers).every((importer) => isEmpty(importer.specifiers ?? {}) && isEmpty(importer.dependencies ?? {}))
}

export async function writeLockfiles (
  opts: {
    wantedLockfile: Lockfile
    wantedLockfileDir: string
    currentLockfile: Lockfile
    currentLockfileDir: string
    useGitBranchLockfile?: boolean
    mergeGitBranchLockfiles?: boolean
  }
): Promise<void> {
  const wantedLockfileName: string = await getWantedLockfileName(opts)
  const wantedLockfilePath = path.join(opts.wantedLockfileDir, wantedLockfileName)
  const currentLockfilePath = path.join(opts.currentLockfileDir, 'lock.yaml')

  const normalizeOpts = {
    forceSharedFormat: true,
  }
  const wantedLockfileToStringify = convertToLockfileFile(opts.wantedLockfile, normalizeOpts)
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
    return
  }

  logger.debug({
    message: `\`${WANTED_LOCKFILE}\` differs from \`${path.relative(opts.wantedLockfileDir, currentLockfilePath)}\``,
    prefix: opts.wantedLockfileDir,
  })

  const currentLockfileToStringify = convertToLockfileFile(opts.currentLockfile, normalizeOpts)
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
}
