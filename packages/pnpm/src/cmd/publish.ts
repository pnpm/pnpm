import { ImporterManifest } from '@pnpm/types'
import cpFile = require('cp-file')
import fg = require('fast-glob')
import fs = require('mz/fs')
import path = require('path')
import R = require('ramda')
import rimraf = require('rimraf-then')
import writeJsonFile = require('write-json-file')
import readImporterManifest from '../readImporterManifest'
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
  const publishManifest = makePublishManifest(manifest)
  const replaceManifest = fileName !== 'package.json' || !R.equals(manifest, publishManifest)
  if (replaceManifest) {
    await rimraf(path.join(prefix, fileName))
    await writeJsonFile(path.join(prefix, 'package.json'), publishManifest)
  }
  await fn()
  if (replaceManifest) {
    await rimraf(path.join(prefix, 'package.json'))
    await writeImporterManifest(manifest, true)
  }
  await Promise.all(
    copiedLicenses.map((copiedLicense) => fs.unlink(copiedLicense))
  )
}

function makePublishManifest (originalManifest: ImporterManifest) {
  if (!originalManifest.publishConfig) return originalManifest
  const publishManifest = { ...originalManifest }
  if (originalManifest.publishConfig.main) {
    publishManifest.main = originalManifest.publishConfig.main
  }
  if (originalManifest.publishConfig.module) {
    publishManifest.module = originalManifest.publishConfig.module
  }
  if (originalManifest.publishConfig.typings) {
    publishManifest.typings = originalManifest.publishConfig.typings
  }
  if (originalManifest.publishConfig.types) {
    publishManifest.types = originalManifest.publishConfig.types
  }
  return publishManifest
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
