import { promises as fs } from 'fs'
import path from 'path'
import { PackageFilesIndex } from '@pnpm/cafs'
import { globalInfo, globalWarn } from '@pnpm/logger'
import rimraf from '@zkochan/rimraf'
import loadJsonFile from 'load-json-file'
import ssri from 'ssri'

const BIG_ONE = BigInt(1) as unknown

export default async function prune (storeDir: string) {
  const cafsDir = path.join(storeDir, 'files')
  await rimraf(path.join(storeDir, 'metadata'))
  await rimraf(path.join(storeDir, 'metadata-full'))
  await rimraf(path.join(storeDir, 'tmp'))
  globalInfo('Removed all cached metadata files')
  const pkgIndexFiles = [] as string[]
  const removedHashes = new Set<string>()
  const dirs = (await fs.readdir(cafsDir, { withFileTypes: true }))
    .filter(entry => entry.isDirectory())
    .map(dir => dir.name)
  let fileCounter = 0
  for (const dir of dirs) {
    const subdir = path.join(cafsDir, dir)
    for (const fileName of await fs.readdir(subdir)) {
      const filePath = path.join(subdir, fileName)
      if (fileName.endsWith('-index.json')) {
        pkgIndexFiles.push(filePath)
        continue
      }
      const stat = await fs.stat(filePath)
      if (stat.isDirectory()) {
        globalWarn(`An alien directory is present in the store: ${filePath}`)
        continue
      }
      if (stat.nlink === 1 || stat.nlink === BIG_ONE) {
        await fs.unlink(filePath)
        fileCounter++
        removedHashes.add(ssri.fromHex(`${dir}${fileName}`, 'sha512').toString())
      }
    }
  }
  globalInfo(`Removed ${fileCounter} file${fileCounter === 1 ? '' : 's'}`)

  let pkgCounter = 0
  for (const pkgIndexFilePath of pkgIndexFiles) {
    const { files: pkgFilesIndex } = await loadJsonFile<PackageFilesIndex>(pkgIndexFilePath)
    if (removedHashes.has(pkgFilesIndex['package.json'].integrity)) {
      await fs.unlink(pkgIndexFilePath)
      pkgCounter++
    }
  }
  globalInfo(`Removed ${pkgCounter} package${pkgCounter === 1 ? '' : 's'}`)
}
