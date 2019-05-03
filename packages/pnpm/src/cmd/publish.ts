import readImporterManifest from '@pnpm/read-importer-manifest'
import path = require('path')
import rimraf = require('rimraf-then')
import writeJsonFile = require('write-json-file')
import { PnpmOptions } from '../types'
import runNpm from './runNpm'

export default async function (
  args: string[],
  opts: PnpmOptions,
  command: string,
) {
  if (args.length && args[0].endsWith('.tgz')) {
    await runNpm(['publish', ...args])
    return
  }
  const prefix = args.length && args[0] || process.cwd()
  const { fileName, manifest, writeImporterManifest } = await readImporterManifest(prefix)
  const exoticManifestFormat = fileName !== 'package.json'
  if (exoticManifestFormat) {
    await rimraf(path.join(prefix, fileName))
    await writeJsonFile(path.join(prefix, 'package.json'), manifest)
  }
  const { status } = await runNpm(['publish', ...opts.argv.original.slice(1)])
  if (exoticManifestFormat) {
    await rimraf(path.join(prefix, 'package.json'))
    await writeImporterManifest(manifest, true)
  }
  if (status !== 0) {
    process.exit(status)
  }
}
