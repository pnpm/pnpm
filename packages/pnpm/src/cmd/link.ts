import {
  docsUrl,
  readImporterManifest,
  readImporterManifestOnly,
  tryReadImporterManifest,
} from '@pnpm/cli-utils'
import { UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import { StoreController } from '@pnpm/package-store'
import pLimit from 'p-limit'
import path = require('path')
import pathAbsolute = require('path-absolute')
import R = require('ramda')
import renderHelp = require('render-help')
import {
  install,
  InstallOptions,
  link,
  linkToGlobal,
  LocalPackages,
} from 'supi'
import { cached as createStoreController } from '../createStoreController'
import findWorkspacePackages, { arrayOfLocalPackagesToMap } from '../findWorkspacePackages'
import getConfig from '../getConfig'
import getSaveType from '../getSaveType'
import { PnpmOptions } from '../types'

const installLimit = pLimit(4)

export function types () {
  return R.pick([
    'global-dir',
    'global',
    'only',
    'package-import-method',
    'production',
    'registry',
    'reporter',
    'resolution-strategy',
    'save-dev',
    'save-exact',
    'save-optional',
  ], allTypes)
}

export const commandNames = ['link', 'ln']

export function help () {
  return renderHelp({
    aliases: ['ln'],
    descriptionLists: [
      {
        title: 'Options',

        list: [
          ...UNIVERSAL_OPTIONS,
        ],
      },
    ],
    url: docsUrl('link'),
    usages: [
      'pnpm link (in package dir)',
      'pnpm link <pkg>',
      'pnpm link <dir>',
    ],
  })
}

export async function handler (
  input: string[],
  opts: PnpmOptions,
) {
  const cwd = opts?.dir ?? process.cwd()

  const storeControllerCache = new Map<string, Promise<{dir: string, ctrl: StoreController}>>()
  let workspacePackages
  let localPackages!: LocalPackages
  if (opts.linkWorkspacePackages && opts.workspaceDir) {
    workspacePackages = await findWorkspacePackages(opts.workspaceDir, opts)
    localPackages = arrayOfLocalPackagesToMap(workspacePackages)
  } else {
    localPackages = {}
  }

  const store = await createStoreController(storeControllerCache, opts)
  const linkOpts = Object.assign(opts, {
    localPackages,
    storeController: store.ctrl,
    storeDir: store.dir,
    targetDependenciesField: getSaveType(opts),
  })

  // pnpm link
  if (!input || !input.length) {
    const { manifest, writeImporterManifest } = await tryReadImporterManifest(opts.globalDir, opts)
    const newManifest = await linkToGlobal(cwd, {
      ...linkOpts,
      // A temporary workaround. global bin/prefix are always defined when --global is set
      globalBin: linkOpts.globalBin!,
      globalDir: linkOpts.globalDir!,
      manifest: manifest || {},
    })
    await writeImporterManifest(newManifest)
    return
  }

  const [pkgPaths, pkgNames] = R.partition((inp) => inp.startsWith('.'), input)

  if (pkgNames.length) {
    let globalPkgNames!: string[]
    if (opts.workspaceDir) {
      workspacePackages = await findWorkspacePackages(opts.workspaceDir, opts)

      const pkgsFoundInWorkspace = workspacePackages.filter((pkg) => pkgNames.includes(pkg.manifest.name))
      pkgsFoundInWorkspace.forEach((pkgFromWorkspace) => pkgPaths.push(pkgFromWorkspace.dir))

      if (pkgsFoundInWorkspace.length && !linkOpts.targetDependenciesField) {
        linkOpts.targetDependenciesField = 'dependencies'
      }

      globalPkgNames = pkgNames.filter((pkgName) => !pkgsFoundInWorkspace.some((pkgFromWorkspace) => pkgFromWorkspace.manifest.name === pkgName))
    } else {
      globalPkgNames = pkgNames
    }
    const globalPkgPath = pathAbsolute(opts.globalDir)
    globalPkgNames.forEach((pkgName) => pkgPaths.push(path.join(globalPkgPath, 'node_modules', pkgName)))
  }

  await Promise.all(
    pkgPaths.map((dir) => installLimit(async () => {
      const s = await createStoreController(storeControllerCache, opts)
      await install(
        await readImporterManifestOnly(dir, opts), {
          ...await getConfig(
            { ...opts.cliArgs, 'dir': dir },
            {
              command: ['link'],
              excludeReporter: true,
            },
          ),
          localPackages,
          storeController: s.ctrl,
          storeDir: s.dir,
        } as InstallOptions,
      )
    })),
  )
  const { manifest, writeImporterManifest } = await readImporterManifest(cwd, opts)

  // When running `pnpm link --production ../source`
  // only the `source` project should be pruned using the --production flag.
  // The target directory should keep its existing dependencies.
  // Except the ones that are replaced by the link.
  delete linkOpts.include

  const newManifest = await link(pkgPaths, path.join(cwd, 'node_modules'), {
    ...linkOpts,
    manifest,
  })
  await writeImporterManifest(newManifest)

  await Promise.all(
    Array.from(storeControllerCache.values())
      .map(async (storeControllerPromise) => {
        const storeControllerHolder = await storeControllerPromise
        await storeControllerHolder.ctrl.close()
      }),
  )
}
