import { promises as fs } from 'fs'
import path from 'path'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import { Lockfile, ProjectSnapshot } from '@pnpm/lockfile-types'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import rimraf from '@zkochan/rimraf'
import * as dp from 'dependency-path'
import yaml from 'js-yaml'
import equals from 'ramda/src/equals'
import fromPairs from 'ramda/src/fromPairs'
import isEmpty from 'ramda/src/isEmpty'
import writeFileAtomicCB from 'write-file-atomic'
import logger from './logger'
import { sortLockfileKeys } from './sortLockfileKeys'
import { getWantedLockfileName } from './lockfileName'
import { convertToInlineSpecifiersFormat } from './experiments/inlineSpecifiersLockfileConverters'

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
    useInlineSpecifiersFormat?: boolean
    useGitBranchLockfile?: boolean
    mergeGitBranchLockfiles?: boolean
  }
) {
  const wantedLockfileName: string = await getWantedLockfileName(opts)
  return writeLockfile(wantedLockfileName, pkgPath, wantedLockfile, opts)
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

interface LockfileFormatOptions {
  forceSharedFormat?: boolean
  useInlineSpecifiersFormat?: boolean
}

async function writeLockfile (
  lockfileFilename: string,
  pkgPath: string,
  wantedLockfile: Lockfile,
  opts?: LockfileFormatOptions
) {
  const lockfilePath = path.join(pkgPath, lockfileFilename)

  // empty lockfile is not saved
  if (isEmptyLockfile(wantedLockfile)) {
    return rimraf(lockfilePath)
  }

  const lockfileToStringify = (opts?.useInlineSpecifiersFormat ?? false)
    ? convertToInlineSpecifiersFormat(wantedLockfile) as unknown as Lockfile
    : wantedLockfile

  const yamlDoc = yamlStringify(lockfileToStringify, {
    forceSharedFormat: opts?.forceSharedFormat === true,
    includeEmptySpecifiersField: !opts?.useInlineSpecifiersFormat,
  })

  return writeFileAtomic(lockfilePath, yamlDoc)
}

function yamlStringify (lockfile: Lockfile, opts: NormalizeLockfileOpts) {
  let normalizedLockfile = normalizeLockfile(lockfile, opts)
  normalizedLockfile = sortLockfileKeys(normalizedLockfile)
  return yaml.dump(normalizedLockfile, LOCKFILE_YAML_FORMAT)
}

function isEmptyLockfile (lockfile: Lockfile) {
  return Object.values(lockfile.importers).every((importer) => isEmpty(importer.specifiers ?? {}) && isEmpty(importer.dependencies ?? {}))
}

export type LockfileFile = Omit<Lockfile, 'importers'> & Partial<ProjectSnapshot> & Partial<Pick<Lockfile, 'importers'>>

export interface NormalizeLockfileOpts {
  forceSharedFormat: boolean
  includeEmptySpecifiersField: boolean
}

export function normalizeLockfile (lockfile: Lockfile, opts: NormalizeLockfileOpts) {
  let lockfileToSave!: LockfileFile
  if (!opts.forceSharedFormat && equals(Object.keys(lockfile.importers), ['.'])) {
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
        const normalizedImporter: Partial<ProjectSnapshot> = {}
        if (!isEmpty(importer.specifiers ?? {}) || opts.includeEmptySpecifiersField) {
          normalizedImporter['specifiers'] = importer.specifiers ?? {}
        }
        if (importer.dependenciesMeta != null && !isEmpty(importer.dependenciesMeta)) {
          normalizedImporter['dependenciesMeta'] = importer.dependenciesMeta
        }
        for (const depType of DEPENDENCIES_FIELDS) {
          if (!isEmpty(importer[depType] ?? {})) {
            normalizedImporter[depType] = importer[depType]
          }
        }
        if (importer.publishDirectory) {
          normalizedImporter.publishDirectory = importer.publishDirectory
        }
        acc[alias] = normalizedImporter
        return acc
      }, {}),
    }
    if (isEmpty(lockfileToSave.packages) || (lockfileToSave.packages == null)) {
      delete lockfileToSave.packages
    }
  }
  if (lockfileToSave.time) {
    lockfileToSave.time = pruneTime(lockfileToSave.time, lockfile.importers)
  }
  if ((lockfileToSave.overrides != null) && isEmpty(lockfileToSave.overrides)) {
    delete lockfileToSave.overrides
  }
  if ((lockfileToSave.patchedDependencies != null) && isEmpty(lockfileToSave.patchedDependencies)) {
    delete lockfileToSave.patchedDependencies
  }
  if (lockfileToSave.neverBuiltDependencies != null) {
    if (isEmpty(lockfileToSave.neverBuiltDependencies)) {
      delete lockfileToSave.neverBuiltDependencies
    } else {
      lockfileToSave.neverBuiltDependencies = lockfileToSave.neverBuiltDependencies.sort()
    }
  }
  if (lockfileToSave.onlyBuiltDependencies != null) {
    lockfileToSave.onlyBuiltDependencies = lockfileToSave.onlyBuiltDependencies.sort()
  }
  if (!lockfileToSave.packageExtensionsChecksum) {
    delete lockfileToSave.packageExtensionsChecksum
  }
  return lockfileToSave
}

function pruneTime (time: Record<string, string>, importers: Record<string, ProjectSnapshot>) {
  const rootDepPaths = new Set<string>()
  for (const importer of Object.values(importers)) {
    for (const depType of DEPENDENCIES_FIELDS) {
      for (let [depName, ref] of Object.entries(importer[depType] ?? {})) {
        if (ref['version']) {
          ref = ref['version']
        }
        const suffixStart = ref.indexOf('_')
        const refWithoutPeerSuffix = suffixStart === -1 ? ref : ref.slice(0, suffixStart)
        const depPath = dp.refToRelative(refWithoutPeerSuffix, depName)
        if (!depPath) continue
        rootDepPaths.add(depPath)
      }
    }
  }
  return fromPairs(Object.entries(time).filter(([depPath]) => rootDepPaths.has(depPath)))
}

export default async function writeLockfiles (
  opts: {
    forceSharedFormat?: boolean
    useInlineSpecifiersFormat?: boolean
    wantedLockfile: Lockfile
    wantedLockfileDir: string
    currentLockfile: Lockfile
    currentLockfileDir: string
    useGitBranchLockfile?: boolean
    mergeGitBranchLockfiles?: boolean
  }
) {
  const wantedLockfileName: string = await getWantedLockfileName(opts)
  const wantedLockfilePath = path.join(opts.wantedLockfileDir, wantedLockfileName)
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
  const wantedLockfileToStringify = (opts.useInlineSpecifiersFormat ?? false)
    ? convertToInlineSpecifiersFormat(opts.wantedLockfile) as unknown as Lockfile
    : opts.wantedLockfile
  const normalizeOpts = {
    forceSharedFormat,
    includeEmptySpecifiersField: !opts.useInlineSpecifiersFormat,
  }
  const yamlDoc = yamlStringify(wantedLockfileToStringify, normalizeOpts)

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

  const currentYamlDoc = yamlStringify(opts.currentLockfile, normalizeOpts)

  await Promise.all([
    writeFileAtomic(wantedLockfilePath, yamlDoc),
    (async () => {
      await fs.mkdir(path.dirname(currentLockfilePath), { recursive: true })
      await writeFileAtomic(currentLockfilePath, currentYamlDoc)
    })(),
  ])
}
