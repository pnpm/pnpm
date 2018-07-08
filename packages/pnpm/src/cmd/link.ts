import findUp = require('find-up')
import path = require('path')
import pathAbsolute = require('path-absolute')
import R = require('ramda')
import {
  link,
  linkFromGlobal,
  linkToGlobal,
} from 'supi'
import {WORKSPACE_MANIFEST_FILENAME} from '../constants'
import createStoreController from '../createStoreController'
import findWorkspacePackages from '../findWorkspacePackages'
import {PnpmOptions} from '../types'

export default async (
  input: string[],
  opts: PnpmOptions,
) => {
  const cwd = opts && opts.prefix || process.cwd()

  const store = await createStoreController(opts)
  const linkOpts = Object.assign(opts, {
    store: store.path,
    storeController: store.ctrl,
  })

  // pnpm link
  if (!input || !input.length) {
    await linkToGlobal(cwd, linkOpts)
    return
  }

  const [pkgPaths, pkgNames] = R.partition((inp) => inp.startsWith('.'), input)

  if (pkgNames.length) {
    const workspaceManifestLocation = await findUp(WORKSPACE_MANIFEST_FILENAME)
    let globalPkgNames!: string[]
    if (workspaceManifestLocation) {
      const workspaceRoot = path.dirname(workspaceManifestLocation)
      const pkgs = await findWorkspacePackages(workspaceRoot)

      const pkgsFoundInWorkspace = pkgs.filter((pkg) => pkgNames.indexOf(pkg.manifest.name) !== -1)
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

  await link(pkgPaths, path.join(cwd, 'node_modules'), linkOpts)
}
