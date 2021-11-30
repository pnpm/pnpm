import path from 'path'
import {
  docsUrl,
  getConfig,
  readProjectManifest,
  readProjectManifestOnly,
  tryReadProjectManifest,
} from '@pnpm/cli-utils'
import { UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { Config, types as allTypes } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import findWorkspaceDir from '@pnpm/find-workspace-dir'
import findWorkspacePackages, { arrayOfWorkspacePackagesToMap } from '@pnpm/find-workspace-packages'
import { StoreController } from '@pnpm/package-store'
import { createOrConnectStoreControllerCached, CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import {
  install,
  InstallOptions,
  link,
  LinkFunctionOptions,
  linkToGlobal,
  WorkspacePackages,
} from '@pnpm/core'
import pLimit from 'p-limit'
import pathAbsolute from 'path-absolute'
import pick from 'ramda/src/pick'
import partition from 'ramda/src/partition'
import renderHelp from 'render-help'
import * as installCommand from './install'
import getOptionsFromRootManifest from './getOptionsFromRootManifest'
import getSaveType from './getSaveType'

const isWindows = process.platform === 'win32' || global['FAKE_WINDOWS']
const isFilespec = isWindows ? /^(?:[.]|~[/]|[/\\]|[a-zA-Z]:)/ : /^(?:[.]|~[/]|[/]|[a-zA-Z]:)/
const installLimit = pLimit(4)

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return pick([
    'global-dir',
    'global',
    'only',
    'package-import-method',
    'production',
    'registry',
    'reporter',
    'save-dev',
    'save-exact',
    'save-optional',
    'save-prefix',
    'unsafe-perm',
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
  opts: CreateStoreControllerOptions & Pick<Config,
  | 'bin'
  | 'cliOptions'
  | 'engineStrict'
  | 'saveDev'
  | 'saveOptional'
  | 'saveProd'
  | 'workspaceDir'
  > & Partial<Pick<Config, 'linkWorkspacePackages'>>,
  params?: string[]
) {
  const cwd = process.cwd()

  const storeControllerCache = new Map<string, Promise<{dir: string, ctrl: StoreController}>>()
  let workspacePackagesArr
  let workspacePackages!: WorkspacePackages
  if (opts.workspaceDir) {
    workspacePackagesArr = await findWorkspacePackages(opts.workspaceDir, opts)
    workspacePackages = arrayOfWorkspacePackagesToMap(workspacePackagesArr)
  } else {
    workspacePackages = {}
  }

  const store = await createOrConnectStoreControllerCached(storeControllerCache, opts)
  const linkOpts = Object.assign(opts, {
    storeController: store.ctrl,
    storeDir: store.dir,
    targetDependenciesField: getSaveType(opts),
    workspacePackages,
  })

  // pnpm link
  if ((params == null) || (params.length === 0)) {
    if (path.relative(linkOpts.dir, cwd) === '') {
      throw new PnpmError('LINK_BAD_PARAMS', 'You must provide a parameter')
    }
    const { manifest, writeProjectManifest } = await tryReadProjectManifest(opts.dir, opts)
    const newManifest = await linkToGlobal(cwd, {
      ...linkOpts,
      dir: cwd,
      globalBin: linkOpts.bin,
      globalDir: linkOpts.dir,
      manifest: manifest ?? {},
    })
    await writeProjectManifest(newManifest)
    return
  }

  const [pkgPaths, pkgNames] = partition((inp) => isFilespec.test(inp), params)

  await Promise.all(
    pkgPaths.map(async (dir) => installLimit(async () => {
      const s = await createOrConnectStoreControllerCached(storeControllerCache, opts)
      const config = await getConfig(
        { ...opts.cliOptions, dir: dir },
        {
          excludeReporter: true,
          rcOptionsTypes: installCommand.rcOptionsTypes(),
          workspaceDir: await findWorkspaceDir(dir),
        }
      )
      await install(
        await readProjectManifestOnly(dir, opts), {
          ...config,
          ...getOptionsFromRootManifest(config.rootProjectManifest ?? {}),
          include: {
            dependencies: config.production !== false,
            devDependencies: config.dev !== false,
            optionalDependencies: config.optional !== false,
          },
          storeController: s.ctrl,
          storeDir: s.dir,
          workspacePackages,
        } as InstallOptions
      )
    }))
  )

  if (pkgNames.length > 0) {
    let globalPkgNames!: string[]
    if (opts.workspaceDir) {
      workspacePackagesArr = await findWorkspacePackages(opts.workspaceDir, opts)

      const pkgsFoundInWorkspace = workspacePackagesArr
        .filter(({ manifest }) => manifest.name && pkgNames.includes(manifest.name))
      pkgsFoundInWorkspace.forEach((pkgFromWorkspace) => pkgPaths.push(pkgFromWorkspace.dir))

      if ((pkgsFoundInWorkspace.length > 0) && !linkOpts.targetDependenciesField) {
        linkOpts.targetDependenciesField = 'dependencies'
      }

      globalPkgNames = pkgNames.filter((pkgName) => !pkgsFoundInWorkspace.some((pkgFromWorkspace) => pkgFromWorkspace.manifest.name === pkgName))
    } else {
      globalPkgNames = pkgNames
    }
    const globalPkgPath = pathAbsolute(opts.dir)
    globalPkgNames.forEach((pkgName) => pkgPaths.push(path.join(globalPkgPath, 'node_modules', pkgName)))
  }

  const { manifest, writeProjectManifest } = await readProjectManifest(cwd, opts)

  const linkConfig = await getConfig(
    { ...opts.cliOptions, dir: cwd },
    {
      excludeReporter: true,
      rcOptionsTypes: installCommand.rcOptionsTypes(),
      workspaceDir: await findWorkspaceDir(cwd),
    }
  )
  const storeL = await createOrConnectStoreControllerCached(storeControllerCache, linkConfig)
  const newManifest = await link(pkgPaths, path.join(cwd, 'node_modules'), {
    ...linkConfig,
    targetDependenciesField: linkOpts.targetDependenciesField,
    storeController: storeL.ctrl,
    storeDir: storeL.dir,
    manifest,
  } as LinkFunctionOptions)
  await writeProjectManifest(newManifest)

  await Promise.all(
    Array.from(storeControllerCache.values())
      .map(async (storeControllerPromise) => {
        const storeControllerHolder = await storeControllerPromise
        await storeControllerHolder.ctrl.close()
      })
  )
}
