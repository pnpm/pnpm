import { promises as fs, readFileSync } from 'fs'
import path from 'path'
import { filesIncludeInstallScripts } from '@pnpm/exec.files-include-install-scripts'
import {
  type CafsLocker,
  createCafs,
  getFilePathByModeInCafs,
} from '@pnpm/store.cafs'
import type { Cafs, PackageFilesResponse } from '@pnpm/cafs-types'
import { createIndexedPkgImporter } from '@pnpm/fs.indexed-pkg-importer'
import {
  type ImportIndexedPackage,
  type ImportIndexedPackageAsync,
  type ImportPackageFunction,
  type ImportPackageFunctionAsync,
  type PackageFileInfo,
} from '@pnpm/store-controller-types'
import memoize from 'mem'
import pathTemp from 'path-temp'
import mapValues from 'ramda/src/map'

export { type CafsLocker }

export function createPackageImporterAsync (
  opts: {
    importIndexedPackage?: ImportIndexedPackageAsync
    packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
    cafsDir: string
  }
): ImportPackageFunctionAsync {
  const cachedImporterCreator = opts.importIndexedPackage
    ? () => opts.importIndexedPackage!
    : memoize(createIndexedPkgImporter)
  const packageImportMethod = opts.packageImportMethod
  const gfm = getFlatMap.bind(null, opts.cafsDir)
  return async (to, opts) => {
    const { filesMap, isBuilt } = gfm(opts.filesResponse, opts.sideEffectsCacheKey)
    const willBeBuilt = !isBuilt && (opts.requiresBuild ?? pkgRequiresBuild(filesMap))
    const pkgImportMethod = willBeBuilt
      ? 'clone-or-copy'
      : (opts.filesResponse.packageImportMethod ?? packageImportMethod)
    const impPkg = cachedImporterCreator(pkgImportMethod)
    const importMethod = await impPkg(to, {
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      filesMap,
      resolvedFrom: opts.filesResponse.resolvedFrom,
      force: opts.force,
      keepModulesDir: Boolean(opts.keepModulesDir),
    })
    return { importMethod, isBuilt }
  }
}

function createPackageImporter (
  opts: {
    importIndexedPackage?: ImportIndexedPackage
    packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
    cafsDir: string
  }
): ImportPackageFunction {
  const cachedImporterCreator = opts.importIndexedPackage
    ? () => opts.importIndexedPackage!
    : memoize(createIndexedPkgImporter)
  const packageImportMethod = opts.packageImportMethod
  const gfm = getFlatMap.bind(null, opts.cafsDir)
  return (to, opts) => {
    const { filesMap, isBuilt } = gfm(opts.filesResponse, opts.sideEffectsCacheKey)
    const willBeBuilt = !isBuilt && (opts.requiresBuild ?? pkgRequiresBuild(filesMap))
    const pkgImportMethod = willBeBuilt
      ? 'clone-or-copy'
      : (opts.filesResponse.packageImportMethod ?? packageImportMethod)
    const impPkg = cachedImporterCreator(pkgImportMethod)
    const importMethod = impPkg(to, {
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      filesMap,
      resolvedFrom: opts.filesResponse.resolvedFrom,
      force: opts.force,
      keepModulesDir: Boolean(opts.keepModulesDir),
    })
    return { importMethod, isBuilt }
  }
}

function getFlatMap (
  cafsDir: string,
  filesResponse: PackageFilesResponse,
  targetEngine?: string
): { filesMap: Record<string, string>, isBuilt: boolean } {
  let isBuilt!: boolean
  let filesIndex!: Record<string, PackageFileInfo>
  if (targetEngine && ((filesResponse.sideEffects?.[targetEngine]) != null)) {
    filesIndex = filesResponse.sideEffects?.[targetEngine]
    isBuilt = true
  } else if (!filesResponse.unprocessed) {
    return {
      filesMap: filesResponse.filesIndex,
      isBuilt: false,
    }
  } else {
    filesIndex = filesResponse.filesIndex
    isBuilt = false
  }
  const filesMap = mapValues(({ integrity, mode }) => getFilePathByModeInCafs(cafsDir, integrity, mode), filesIndex)
  return { filesMap, isBuilt }
}

export function createCafsStore (
  storeDir: string,
  opts?: {
    ignoreFile?: (filename: string) => boolean
    importPackage?: ImportIndexedPackage
    packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy'
    cafsLocker?: CafsLocker
  }
): Cafs {
  const cafsDir = path.join(storeDir, 'files')
  const baseTempDir = path.join(storeDir, 'tmp')
  const importPackage = createPackageImporter({
    importIndexedPackage: opts?.importPackage,
    packageImportMethod: opts?.packageImportMethod,
    cafsDir,
  })
  return {
    ...createCafs(cafsDir, opts),
    cafsDir,
    importPackage,
    tempDir: async () => {
      const tmpDir = pathTemp(baseTempDir)
      await fs.mkdir(tmpDir, { recursive: true })
      return tmpDir
    },
  }
}

function pkgRequiresBuild (filesMap: Record<string, string>) {
  return filesIncludeInstallScripts(filesMap) ||
    filesMap['package.json'] && pkgJsonHasInstallScripts(filesMap['package.json'])
}

function pkgJsonHasInstallScripts (file: string): boolean {
  const pkgJson = JSON.parse(readFileSync(file, 'utf8'))
  if (!pkgJson.scripts) return false
  return Boolean(pkgJson.scripts.preinstall) ||
    Boolean(pkgJson.scripts.install) ||
    Boolean(pkgJson.scripts.postinstall)
}
