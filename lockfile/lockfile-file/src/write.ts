import { promises as fs } from 'fs'
import path from 'path'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import { type Lockfile, type ProjectSnapshot } from '@pnpm/lockfile-types'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import rimraf from '@zkochan/rimraf'
import * as dp from '@pnpm/dependency-path'
import yaml from 'js-yaml'
import equals from 'ramda/src/equals'
import pickBy from 'ramda/src/pickBy'
import isEmpty from 'ramda/src/isEmpty'
import mapValues from 'ramda/src/map'
import writeFileAtomicCB from 'write-file-atomic'
import { lockfileLogger as logger } from './logger'
import { sortLockfileKeys } from './sortLockfileKeys'
import { getWantedLockfileName } from './lockfileName'
import { convertToInlineSpecifiersFormat } from './experiments/inlineSpecifiersLockfileConverters'

async function writeFileAtomic (filename: string, data: string) {
  return new Promise<void>((resolve, reject) => {
    writeFileAtomicCB(filename, data, {}, (err?: Error) => {
      (err != null) ? reject(err) : resolve()
    })
  })
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
  // empty lockfile is not saved
  if (isEmptyLockfile(currentLockfile)) {
    await rimraf(path.join(virtualStoreDir, 'lock.yaml'))
    return
  }
  await fs.mkdir(virtualStoreDir, { recursive: true })
  return writeLockfile('lock.yaml', virtualStoreDir, currentLockfile, opts)
}

interface LockfileFormatOptions {
  forceSharedFormat?: boolean
}

async function writeLockfile (
  lockfileFilename: string,
  pkgPath: string,
  wantedLockfile: Lockfile,
  opts?: LockfileFormatOptions
) {
  const lockfilePath = path.join(pkgPath, lockfileFilename)

  const isLockfileV6 = wantedLockfile['lockfileVersion'].toString().startsWith('6.')
  const lockfileToStringify = isLockfileV6
    ? convertToInlineSpecifiersFormat(wantedLockfile) as unknown as Lockfile
    : wantedLockfile

  const yamlDoc = yamlStringify(lockfileToStringify, {
    forceSharedFormat: opts?.forceSharedFormat === true,
    includeEmptySpecifiersField: !isLockfileV6,
  })

  return writeFileAtomic(lockfilePath, yamlDoc)
}

function yamlStringify (lockfile: Lockfile, opts: NormalizeLockfileOpts) {
  let normalizedLockfile = normalizeLockfile(lockfile, opts)
  normalizedLockfile = sortLockfileKeys(normalizedLockfile)
  return yaml.dump(normalizedLockfile, LOCKFILE_YAML_FORMAT)
}

export function isEmptyLockfile (lockfile: Lockfile) {
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
      importers: mapValues((importer) => {
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
        return normalizedImporter as ProjectSnapshot
      }, lockfile.importers),
    }
    if (isEmpty(lockfileToSave.packages) || (lockfileToSave.packages == null)) {
      delete lockfileToSave.packages
    }
  }
  if (lockfileToSave.time) {
    lockfileToSave.time = (lockfileToSave.lockfileVersion.toString().startsWith('6.') ? pruneTimeInLockfileV6 : pruneTime)(lockfileToSave.time, lockfile.importers)
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

function pruneTimeInLockfileV6 (time: Record<string, string>, importers: Record<string, ProjectSnapshot>): Record<string, string> {
  const rootDepPaths = new Set<string>()
  for (const importer of Object.values(importers)) {
    for (const depType of DEPENDENCIES_FIELDS) {
      for (let [depName, ref] of Object.entries(importer[depType] ?? {})) {
        // @ts-expect-error
        if (ref['version']) ref = ref['version']
        const suffixStart = ref.indexOf('(')
        const refWithoutPeerSuffix = suffixStart === -1 ? ref : ref.slice(0, suffixStart)
        const depPath = refToRelative(refWithoutPeerSuffix, depName)
        if (!depPath) continue
        rootDepPaths.add(depPath)
      }
    }
  }
  return pickBy((_, depPath) => rootDepPaths.has(depPath), time)
}

function refToRelative (
  reference: string,
  pkgName: string
): string | null {
  if (reference.startsWith('link:')) {
    return null
  }
  if (reference.startsWith('file:')) {
    return reference
  }
  if (!reference.includes('/') || !reference.replace(/(\([^)]+\))+$/, '').includes('/')) {
    return `/${pkgName}@${reference}`
  }
  return reference
}

function pruneTime (time: Record<string, string>, importers: Record<string, ProjectSnapshot>): Record<string, string> {
  const rootDepPaths = new Set<string>()
  for (const importer of Object.values(importers)) {
    for (const depType of DEPENDENCIES_FIELDS) {
      for (let [depName, ref] of Object.entries(importer[depType] ?? {})) {
        // @ts-expect-error
        if (ref['version']) ref = ref['version']
        const suffixStart = ref.indexOf('_')
        const refWithoutPeerSuffix = suffixStart === -1 ? ref : ref.slice(0, suffixStart)
        const depPath = dp.refToRelative(refWithoutPeerSuffix, depName)
        if (!depPath) continue
        rootDepPaths.add(depPath)
      }
    }
  }
  return pickBy((t, depPath) => rootDepPaths.has(depPath), time)
}

export async function writeLockfiles (
  opts: {
    forceSharedFormat?: boolean
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

  const forceSharedFormat = opts?.forceSharedFormat === true
  const isLockfileV6 = opts.wantedLockfile.lockfileVersion.toString().startsWith('6.')
  const wantedLockfileToStringify = isLockfileV6
    ? convertToInlineSpecifiersFormat(opts.wantedLockfile) as unknown as Lockfile
    : opts.wantedLockfile
  const normalizeOpts = {
    forceSharedFormat,
    includeEmptySpecifiersField: !isLockfileV6,
  }
  const yamlDoc = yamlStringify(wantedLockfileToStringify, normalizeOpts)

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

  const currentLockfileToStringify = opts.wantedLockfile.lockfileVersion.toString().startsWith('6.')
    ? convertToInlineSpecifiersFormat(opts.currentLockfile) as unknown as Lockfile
    : opts.currentLockfile
  const currentYamlDoc = yamlStringify(currentLockfileToStringify, normalizeOpts)

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
