import { type Dirent, promises as fs } from 'fs'
import util from 'util'
import path from 'path'
import { type PackageFilesIndex } from '@pnpm/store.cafs'
import { globalInfo, globalWarn } from '@pnpm/logger'
import rimraf from '@zkochan/rimraf'
import loadJsonFile from 'load-json-file'
import ssri from 'ssri'

const BIG_ONE = BigInt(1) as unknown

export interface PruneOptions {
  cacheDir: string
  storeDir: string
}

export async function prune ({ cacheDir, storeDir }: PruneOptions, removeAlienFiles?: boolean): Promise<void> {
  const cafsDir = path.join(storeDir, 'files')
  await Promise.all([
    rimraf(path.join(cacheDir, 'metadata')),
    rimraf(path.join(cacheDir, 'metadata-full')),
    rimraf(path.join(cacheDir, 'metadata-v1.1')),
    rimraf(path.join(cacheDir, 'metadata-v1.2')),
  ])
  await rimraf(path.join(storeDir, 'tmp'))
  globalInfo('Removed all cached metadata files')
  const pkgIndexFiles = [] as string[]
  const removedHashes = new Set<string>()
  const dirs = await getSubdirsSafely(cafsDir)
  let fileCounter = 0
  await Promise.all(dirs.map(async (dir) => {
    const subdir = path.join(cafsDir, dir)
    await Promise.all((await fs.readdir(subdir)).map(async (fileName) => {
      const filePath = path.join(subdir, fileName)
      if (fileName.endsWith('.json')) {
        pkgIndexFiles.push(filePath)
        return
      }
      const stat = await fs.stat(filePath)
      if (stat.isDirectory()) {
        if (removeAlienFiles) {
          await rimraf(filePath)
          globalWarn(`An alien directory has been removed from the store: ${filePath}`)
          fileCounter++
          return
        } else {
          globalWarn(`An alien directory is present in the store: ${filePath}`)
          return
        }
      }
      if (stat.nlink === 1 || stat.nlink === BIG_ONE) {
        await fs.unlink(filePath)
        fileCounter++
        removedHashes.add(ssri.fromHex(`${dir}${fileName}`, 'sha512').toString())
      }
    }))
  }))
  globalInfo(`Removed ${fileCounter} file${fileCounter === 1 ? '' : 's'}`)

  let pkgCounter = 0
  await Promise.all(pkgIndexFiles.map(async (pkgIndexFilePath) => {
    const { files: pkgFilesIndex } = await loadJsonFile<PackageFilesIndex>(pkgIndexFilePath)
    if (removedHashes.has(pkgFilesIndex['package.json'].integrity)) {
      await fs.unlink(pkgIndexFilePath)
      pkgCounter++
    }
  }))
  globalInfo(`Removed ${pkgCounter} package${pkgCounter === 1 ? '' : 's'}`)
}

async function getSubdirsSafely (dir: string): Promise<string[]> {
  let entries: Dirent[]
  try {
    entries = await fs.readdir(dir, { withFileTypes: true }) as Dirent[]
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return []
    }
    throw err
  }
  return entries
    .filter(entry => entry.isDirectory())
    .map(dir => dir.name)
}
