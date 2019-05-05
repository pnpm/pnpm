import readImporterManifest from '@pnpm/read-importer-manifest'
import cpFile = require('cp-file')
import fg = require('fast-glob')
import fs = require('mz/fs')
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

  let _status!: number
  await fakeRegularManifest(prefix, opts.workspacePrefix || prefix, async () => {
    const { status } = await runNpm(['publish', ...opts.argv.original.slice(1)])
    _status = status
  })
  if (_status !== 0) {
    process.exit(_status)
  }
}

export async function pack (
  args: string[],
  opts: PnpmOptions,
  command: string,
) {
  let _status!: number
  await fakeRegularManifest(opts.prefix, opts.workspacePrefix || opts.prefix, async () => {
    const { status } = await runNpm(['pack', ...opts.argv.original.slice(1)])
    _status = status
  })
  if (_status !== 0) {
    process.exit(_status)
  }
}

const LICENSE_GLOB = 'LICEN{S,C}E{,.*}'
const findLicenses = fg.bind(fg, [LICENSE_GLOB]) as (opts: { cwd: string }) => Promise<string[]>

async function fakeRegularManifest (prefix: string, workspacePrefix: string, fn: () => Promise<void>) {
  // If a workspace package has no License of its own,
  // license files from the root of the workspace are used
  const copiedLicenses: string[] = prefix !== workspacePrefix && (await findLicenses({ cwd: prefix })).length === 0
    ? await copyLicenses(workspacePrefix, prefix) : []

  const { fileName, manifest, writeImporterManifest } = await readImporterManifest(prefix)
  const exoticManifestFormat = fileName !== 'package.json'
  if (exoticManifestFormat) {
    await rimraf(path.join(prefix, fileName))
    await writeJsonFile(path.join(prefix, 'package.json'), manifest)
  }
  await fn()
  if (exoticManifestFormat) {
    await rimraf(path.join(prefix, 'package.json'))
    await writeImporterManifest(manifest, true)
  }
  await Promise.all(
    copiedLicenses.map((copiedLicense) => fs.unlink(copiedLicense))
  )
}

async function copyLicenses (sourceDir: string, destDir: string) {
  const licenses = await findLicenses({ cwd: sourceDir })
  if (licenses.length === 0) return []

  const copiedLicenses: string[] = []
  await Promise.all(
    licenses
      .map((licenseRelPath) => path.join(sourceDir, licenseRelPath))
      .map((licensePath) => {
        const licenseCopyDest = path.join(destDir, path.basename(licensePath))
        copiedLicenses.push(licenseCopyDest)
        return cpFile(licensePath, licenseCopyDest)
      })
  )
  return copiedLicenses
}
