import { StoreController } from '@pnpm/package-store'
import pLimit from 'p-limit'
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
import getConfig from '../getConfig'
import readImporterManifest, {
  readImporterManifestOnly,
  tryReadImporterManifest,
} from '../readImporterManifest'
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
    workspacePackages = await findWorkspacePackages(opts.workspacePrefix, opts)
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
    const { manifest, writeImporterManifest } = await tryReadImporterManifest(opts.globalPrefix, opts)
    const newManifest = await linkToGlobal(cwd, {
      ...linkOpts,
      // A temporary workaround. global bin/prefix are always defined when --global is set
      globalBin: linkOpts.globalBin!,
      globalPrefix: linkOpts.globalPrefix!,
      manifest: manifest || {},
    })
    await writeImporterManifest(newManifest)
    return
  }

  const [pkgPaths, pkgNames] = R.partition((inp) => inp.startsWith('.'), input)

  if (pkgNames.length) {
    let globalPkgNames!: string[]
    if (opts.workspacePrefix) {
      workspacePackages = await findWorkspacePackages(opts.workspacePrefix, opts)

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
        await readImporterManifestOnly(prefix, opts), {
          ...await getConfig(
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
