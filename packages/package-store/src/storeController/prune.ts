import { PackageFilesIndex } from '@pnpm/cafs'
import { globalInfo } from '@pnpm/logger'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import loadJsonFile = require('load-json-file')
import fs = require('mz/fs')
import ssri = require('ssri')

const BIG_ONE = BigInt(1) as unknown

export default async function prune (storeDir: string) {
  const cafsDir = path.join(storeDir, 'files')
  await rimraf(path.join(storeDir, 'metadata'))
  await rimraf(path.join(storeDir, 'metadata-full'))
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
