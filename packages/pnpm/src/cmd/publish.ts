import PnpmError from '@pnpm/error'
import { tryReadImporterManifest } from '@pnpm/read-importer-manifest'
import { Dependencies, ImporterManifest } from '@pnpm/types'
import rimraf = require('@zkochan/rimraf')
import cpFile = require('cp-file')
import fg = require('fast-glob')
import fs = require('mz/fs')
import path = require('path')
import R = require('ramda')
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
  const dir = args.length && args[0] || process.cwd()

  let _status!: number
  await fakeRegularManifest(
    {
      dir,
      engineStrict: opts.engineStrict,
      workspaceDir: opts.workspaceDir || dir,
    },
    async () => {
      const { status } = await runNpm(['publish', ...opts.argv.original.slice(1)])
      _status = status
    },
  )
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
  await fakeRegularManifest({
    dir: opts.dir,
    engineStrict: opts.engineStrict,
    workspaceDir: opts.workspaceDir || opts.dir,
  }, async () => {
    const { status } = await runNpm(['pack', ...opts.argv.original.slice(1)])
    _status = status
  })
  if (_status !== 0) {
    process.exit(_status)
  }
}

const LICENSE_GLOB = 'LICEN{S,C}E{,.*}'
const findLicenses = fg.bind(fg, [LICENSE_GLOB]) as (opts: { cwd: string }) => Promise<string[]>

async function fakeRegularManifest (
  opts: {
    engineStrict?: boolean,
    dir: string,
    workspaceDir: string,
  },
  fn: () => Promise<void>,
) {
  // If a workspace package has no License of its own,
  // license files from the root of the workspace are used
  const copiedLicenses: string[] = opts.dir !== opts.workspaceDir && (await findLicenses({ cwd: opts.dir })).length === 0
    ? await copyLicenses(opts.workspaceDir, opts.dir) : []

  const { fileName, manifest, writeImporterManifest } = await readImporterManifest(opts.dir, opts)
  const publishManifest = await makePublishManifest(opts.dir, manifest)
  const replaceManifest = fileName !== 'package.json' || !R.equals(manifest, publishManifest)
  if (replaceManifest) {
    await rimraf(path.join(opts.dir, fileName))
    await writeJsonFile(path.join(opts.dir, 'package.json'), publishManifest)
  }
  await fn()
  if (replaceManifest) {
    await rimraf(path.join(opts.dir, 'package.json'))
    await writeImporterManifest(manifest, true)
  }
  await Promise.all(
    copiedLicenses.map((copiedLicense) => fs.unlink(copiedLicense))
  )
}

async function makePublishManifest (dir: string, originalManifest: ImporterManifest) {
  const publishManifest = {
    ...originalManifest,
    dependencies: await makePublishDependencies(dir, originalManifest.dependencies),
    optionalDependencies: await makePublishDependencies(dir, originalManifest.optionalDependencies),
  }
  if (originalManifest.publishConfig) {
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
  }
  return publishManifest
}

async function makePublishDependencies (dir: string, dependencies: Dependencies | undefined) {
  if (!dependencies) return dependencies
  const publishDependencies: Dependencies = R.fromPairs(
    await Promise.all(
      R.toPairs(dependencies)
        .map(async ([depName, depSpec]) => [
          depName,
          await makePublishDependency(depName, depSpec, dir),
        ]),
    ) as any, // tslint:disable-line
  )
  return publishDependencies
}

async function makePublishDependency (depName: string, depSpec: string, dir: string) {
  if (!depSpec.startsWith('workspace:')) {
    return depSpec
  }
  if (depSpec === 'workspace:*') {
    const { manifest } = await tryReadImporterManifest(path.join(dir, 'node_modules', depName))
    if (!manifest || !manifest.version) {
      throw new PnpmError(
        'CANNOT_RESOLVE_WORKSPACE_PROTOCOL',
        `Cannot resolve workspace protocol of dependency "${depName}" ` +
          `because this dependency is not installed. Try running "pnpm install".`,
      )
    }
    return manifest.version
  }
  return depSpec.substr(10)
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
