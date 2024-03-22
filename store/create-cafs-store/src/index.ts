import '@total-typescript/ts-reset'
import path from 'node:path'
import { promises as fs, readFileSync } from 'node:fs'

import memoize from 'mem'
import pathTemp from 'path-temp'
import mapValues from 'ramda/src/map'

import { filesIncludeInstallScripts } from '@pnpm/exec.files-include-install-scripts'
import {
  createCafs,
  getFilePathByModeInCafs,
} from '@pnpm/store.cafs'
import type {
  Cafs,
  CafsLocker,
  PackageFileInfo,
  PackageFilesResponse,
  ImportIndexedPackage,
  ImportPackageFunction,
  ImportIndexedPackageAsync,
  ImportPackageFunctionAsync,
} from '@pnpm/types'
import { createIndexedPkgImporter } from '@pnpm/fs.indexed-pkg-importer'

export { type CafsLocker }

export function createPackageImporterAsync(opts: {
  importIndexedPackage?: ImportIndexedPackageAsync | undefined
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy' | undefined
  cafsDir: string
}): ImportPackageFunctionAsync {
  const cachedImporterCreator = opts.importIndexedPackage
    ? (): ImportIndexedPackageAsync | undefined => {
      return opts.importIndexedPackage;
    }
    : memoize(createIndexedPkgImporter)

  const packageImportMethod = opts.packageImportMethod

  const gfm = getFlatMap.bind(null, opts.cafsDir)

  return async (to, opts) => {
    const { filesMap, isBuilt } = gfm(
      opts.filesResponse,
      opts.sideEffectsCacheKey
    )

    const willBeBuilt =
      !isBuilt && (opts.requiresBuild ?? pkgRequiresBuild(filesMap))

    const pkgImportMethod = willBeBuilt
      ? 'clone-or-copy'
      : opts.filesResponse?.packageImportMethod ?? packageImportMethod

    const impPkg = cachedImporterCreator(pkgImportMethod)

    const importMethod = await impPkg?.(to, {
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      filesMap,
      resolvedFrom: opts.filesResponse?.resolvedFrom,
      force: opts.force,
      keepModulesDir: Boolean(opts.keepModulesDir),
    })

    return { importMethod, isBuilt }
  }
}

function createPackageImporter(opts: {
  importIndexedPackage?: ImportIndexedPackage | undefined
  packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'clone' | 'clone-or-copy' | undefined
  cafsDir: string
}): ImportPackageFunction {
  const cachedImporterCreator = opts.importIndexedPackage
    ? (): ImportIndexedPackage | undefined => opts.importIndexedPackage
    : memoize(createIndexedPkgImporter)

  const packageImportMethod = opts.packageImportMethod

  const gfm = getFlatMap.bind(null, opts.cafsDir)

  return (to, opts) => {
    const { filesMap, isBuilt } = gfm(
      opts.filesResponse,
      opts.sideEffectsCacheKey
    )

    const willBeBuilt =
      !isBuilt && (opts.requiresBuild ?? pkgRequiresBuild(filesMap))

    const pkgImportMethod = willBeBuilt
      ? 'clone-or-copy'
      : opts.filesResponse?.packageImportMethod ?? packageImportMethod

    const impPkg = cachedImporterCreator(pkgImportMethod)

    const importMethod = impPkg?.(to, {
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      filesMap,
      resolvedFrom: opts.filesResponse?.resolvedFrom,
      force: opts.force,
      keepModulesDir: Boolean(opts.keepModulesDir),
    })

    return { importMethod, isBuilt }
  }
}

function getFlatMap(
  cafsDir: string,
  filesResponse: PackageFilesResponse | undefined,
  targetEngine?: string | undefined
): { filesMap?: Record<string, string> | undefined; isBuilt: boolean } {
  let isBuilt!: boolean

  let filesIndex!: Record<string, PackageFileInfo>

  if (targetEngine && filesResponse?.sideEffects?.[targetEngine] != null) {
    filesIndex = filesResponse.sideEffects?.[targetEngine]
    isBuilt = true
  } else if (!filesResponse?.unprocessed) {
    return {
      filesMap: filesResponse?.filesIndex,
      isBuilt: false,
    }
  } else {
    filesIndex = filesResponse.filesIndex
    isBuilt = false
  }

  const filesMap = mapValues(
    ({ integrity, mode }: PackageFileInfo): string => {
      return getFilePathByModeInCafs(cafsDir, integrity, mode);
    },
    filesIndex
  )

  return { filesMap, isBuilt }
}

export function createCafsStore(
  storeDir: string,
  opts?: {
    ignoreFile?: ((filename: string) => boolean) | undefined
    importPackage?: ImportIndexedPackage | undefined
    packageImportMethod?:
      | 'auto'
      | 'hardlink'
      | 'copy'
      | 'clone'
      | 'clone-or-copy' | undefined
    cafsLocker?: CafsLocker | undefined
  } | undefined
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

function pkgRequiresBuild(filesMap: Record<string, string> | undefined): boolean {
  return (
    filesIncludeInstallScripts(filesMap) ||
    (typeof filesMap?.['package.json'] !== 'undefined' &&
      pkgJsonHasInstallScripts(filesMap['package.json']))
  )
}

function pkgJsonHasInstallScripts(file: string): boolean {
  const pkgJson = JSON.parse(readFileSync(file, 'utf8'))

  if (typeof pkgJson !== 'object' || pkgJson === null || !('scripts' in pkgJson) || typeof pkgJson.scripts !== 'object' || pkgJson.scripts === null) {
    return false
  }

  return (
    // @ts-ignore
    Boolean(pkgJson.scripts.preinstall) ||
    // @ts-ignore
    Boolean(pkgJson.scripts.install) ||
    // @ts-ignore
    Boolean(pkgJson.scripts.postinstall)
  )
}
