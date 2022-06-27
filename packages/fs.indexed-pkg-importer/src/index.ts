import { constants, promises as fs, Stats } from 'fs'
import path from 'path'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { packageImportMethodLogger } from '@pnpm/core-loggers'
import pLimit from 'p-limit'
import exists from 'path-exists'
import importIndexedDir, { ImportFile } from './importIndexedDir'

const limitLinking = pLimit(16)

type FilesMap = Record<string, string>

interface ImportOptions {
  filesMap: FilesMap
  force: boolean
  fromStore: boolean
}

type ImportFunction = (to: string, opts: ImportOptions) => Promise<string | undefined>

export function createIndexedPkgImporter (
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
): ImportFunction {
  const importPackage = createImportPackage(packageImportMethod)
  return async (to, opts) => limitLinking(async () => importPackage(to, opts))
}

function createImportPackage (packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy') {
  // this works in the following way:
  // - hardlink: hardlink the packages, no fallback
  // - clone: clone the packages, no fallback
  // - auto: try to clone or hardlink the packages, if it fails, fallback to copy
  // - copy: copy the packages, do not try to link them first
  switch (packageImportMethod ?? 'auto') {
  case 'clone':
    packageImportMethodLogger.debug({ method: 'clone' })
    return clonePkg
  case 'hardlink':
    packageImportMethodLogger.debug({ method: 'hardlink' })
    return hardlinkPkg.bind(null, linkOrCopy)
  case 'auto': {
    return createAutoImporter()
  }
  case 'clone-or-copy':
    return createCloneOrCopyImporter()
  case 'copy':
    packageImportMethodLogger.debug({ method: 'copy' })
    return copyPkg
  default:
    throw new Error(`Unknown package import method ${packageImportMethod as string}`)
  }
}

function createAutoImporter (): ImportFunction {
  let auto = initialAuto

  return async (to, opts) => auto(to, opts)

  async function initialAuto (
    to: string,
    opts: ImportOptions
  ): Promise<string | undefined> {
    try {
      if (!await clonePkg(to, opts)) return undefined
      packageImportMethodLogger.debug({ method: 'clone' })
      auto = clonePkg
      return 'clone'
    } catch (err: any) { // eslint-disable-line
      // ignore
    }
    try {
      if (!await hardlinkPkg(fs.link, to, opts)) return undefined
      packageImportMethodLogger.debug({ method: 'hardlink' })
      auto = hardlinkPkg.bind(null, linkOrCopy)
      return 'hardlink'
    } catch (err: any) { // eslint-disable-line
      if (err.message.startsWith('EXDEV: cross-device link not permitted')) {
        globalWarn(err.message)
        globalInfo('Falling back to copying packages from store')
        packageImportMethodLogger.debug({ method: 'copy' })
        auto = copyPkg
        return auto(to, opts)
      }
      // We still choose hard linking that will fall back to copying in edge cases.
      packageImportMethodLogger.debug({ method: 'hardlink' })
      auto = hardlinkPkg.bind(null, linkOrCopy)
      return auto(to, opts)
    }
  }
}

function createCloneOrCopyImporter (): ImportFunction {
  let auto = initialAuto

  return async (to, opts) => auto(to, opts)

  async function initialAuto (
    to: string,
    opts: ImportOptions
  ): Promise<string | undefined> {
    try {
      if (!await clonePkg(to, opts)) return undefined
      packageImportMethodLogger.debug({ method: 'clone' })
      auto = clonePkg
      return 'clone'
    } catch (err: any) { // eslint-disable-line
      // ignore
    }
    packageImportMethodLogger.debug({ method: 'copy' })
    auto = copyPkg
    return auto(to, opts)
  }
}

async function clonePkg (
  to: string,
  opts: ImportOptions
) {
  const pkgJsonPath = path.join(to, 'package.json')

  if (!opts.fromStore || opts.force || !await exists(pkgJsonPath)) {
    await importIndexedDir(cloneFile, to, opts.filesMap)
    return 'clone'
  }
  return undefined
}

async function cloneFile (from: string, to: string) {
  await fs.copyFile(from, to, constants.COPYFILE_FICLONE_FORCE)
}

async function hardlinkPkg (
  importFile: ImportFile,
  to: string,
  opts: ImportOptions
) {
  if (
    !opts.fromStore ||
    opts.force ||
    !await pkgLinkedToStore(opts.filesMap, to)
  ) {
    await importIndexedDir(importFile, to, opts.filesMap)
    return 'hardlink'
  }
  return undefined
}

async function linkOrCopy (existingPath: string, newPath: string) {
  try {
    await fs.link(existingPath, newPath)
  } catch (err: any) { // eslint-disable-line
    // If a hard link to the same file already exists
    // then trying to copy it will make an empty file from it.
    if (err['code'] === 'EEXIST') return
    // In some VERY rare cases (1 in a thousand), hard-link creation fails on Windows.
    // In that case, we just fall back to copying.
    // This issue is reproducible with "pnpm add @material-ui/icons@4.9.1"
    await fs.copyFile(existingPath, newPath)
  }
}

async function pkgLinkedToStore (
  filesMap: FilesMap,
  to: string
) {
  if (filesMap['package.json']) {
    if (await isSameFile('package.json', to, filesMap)) {
      return true
    }
  } else {
    // An injected package might not have a package.json.
    // This will probably only even happen in a Bit workspace.
    const [anyFile] = Object.keys(filesMap)
    if (await isSameFile(anyFile, to, filesMap)) return true
  }
  return false
}

async function isSameFile (filename: string, linkedPkgDir: string, filesMap: FilesMap) {
  const linkedFile = path.join(linkedPkgDir, filename)
  let stats0!: Stats
  try {
    stats0 = await fs.stat(linkedFile)
  } catch (err: any) { // eslint-disable-line
    if (err.code === 'ENOENT') return false
  }
  const stats1 = await fs.stat(filesMap[filename])
  if (stats0.ino === stats1.ino) return true
  globalInfo(`Relinking ${linkedPkgDir} from the store`)
  return false
}

export async function copyPkg (
  to: string,
  opts: ImportOptions
) {
  const pkgJsonPath = path.join(to, 'package.json')

  if (!opts.fromStore || opts.force || !await exists(pkgJsonPath)) {
    await importIndexedDir(fs.copyFile, to, opts.filesMap)
    return 'copy'
  }
  return undefined
}
