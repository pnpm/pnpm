import { StoreController } from '@pnpm/package-store'
import { safeReadPackageFromDir } from '@pnpm/utils'
import pLimit = require('p-limit')
import path = require('path')
import pathAbsolute = require('path-absolute')
import R = require('ramda')
import {
  install,
  InstallOptions,
  link,
  linkToGlobal,
  LocalPackages,
} from 'supi'
import { cached as createStoreController } from '../createStoreController'
import findWorkspacePackages, { arrayOfLocalPackagesToMap } from '../findWorkspacePackages'
import getConfigs from '../getConfigs'
import { readImporterManifestFromDir } from '../readImporterManifest'
import { PnpmOptions } from '../types'

const installLimit = pLimit(4)

export default async (
  input: string[],
  opts: PnpmOptions,
) => {
  const cwd = opts && opts.prefix || process.cwd()

  const storeControllerCache = new Map<string, Promise<{path: string, ctrl: StoreController}>>()
  let workspacePackages
  let localPackages!: LocalPackages
  if (opts.linkWorkspacePackages && opts.workspacePrefix) {
    workspacePackages = await findWorkspacePackages(opts.workspacePrefix)
    localPackages = arrayOfLocalPackagesToMap(workspacePackages)
  } else {
    localPackages = {}
  }

  const store = await createStoreController(storeControllerCache, opts)
  const linkOpts = Object.assign(opts, {
    localPackages,
    store: store.path,
    storeController: store.ctrl,
  })

  // pnpm link
  if (!input || !input.length) {
    await linkToGlobal(cwd, {
      ...linkOpts,
      manifest: await safeReadPackageFromDir(opts.globalPrefix) || {}
    })
    return
  }

  const [pkgPaths, pkgNames] = R.partition((inp) => inp.startsWith('.'), input)

  if (pkgNames.length) {
    let globalPkgNames!: string[]
    if (opts.workspacePrefix) {
      workspacePackages = await findWorkspacePackages(opts.workspacePrefix)

      const pkgsFoundInWorkspace = workspacePackages.filter((pkg) => pkgNames.includes(pkg.manifest.name))
      pkgsFoundInWorkspace.forEach((pkgFromWorkspace) => pkgPaths.push(pkgFromWorkspace.path))

      if (pkgsFoundInWorkspace.length && !linkOpts.saveDev && !linkOpts.saveProd && !linkOpts.saveOptional) {
        linkOpts.saveProd = true
      }

      globalPkgNames = pkgNames.filter((pkgName) => !pkgsFoundInWorkspace.some((pkgFromWorkspace) => pkgFromWorkspace.manifest.name === pkgName))
    } else {
      globalPkgNames = pkgNames
    }
    const globalPkgPath = pathAbsolute(opts.globalPrefix)
    globalPkgNames.forEach((pkgName) => pkgPaths.push(path.join(globalPkgPath, 'node_modules', pkgName)))
  }

  await Promise.all(
    pkgPaths.map((prefix) => installLimit(async () => {
      const s = await createStoreController(storeControllerCache, opts)
      await install(
        await readImporterManifestFromDir(prefix), {
          ...await getConfigs(
            { ...opts.cliArgs, prefix },
            {
              command: ['link'],
              excludeReporter: true,
            },
          ),
          localPackages,
          store: s.path,
          storeController: s.ctrl,
        } as InstallOptions,
      )
    })),
  )
  await link(pkgPaths, path.join(cwd, 'node_modules'), {
    ...linkOpts,
    manifest: await readImporterManifestFromDir(cwd),
  })

  await Promise.all(
    Array.from(storeControllerCache.values())
      .map(async (storeControllerPromise) => {
        const storeControllerHolder = await storeControllerPromise
        await storeControllerHolder.ctrl.close()
      }),
  )
}
