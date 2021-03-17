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
import findWorkspaceDir from '@pnpm/find-workspace-dir'
import findWorkspacePackages, { arrayOfWorkspacePackagesToMap } from '@pnpm/find-workspace-packages'
import { StoreController } from '@pnpm/package-store'
import { createOrConnectStoreControllerCached, CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import {
  install,
  InstallOptions,
  link,
  linkToGlobal,
  WorkspacePackages,
} from 'supi'
import pLimit from 'p-limit'
import pathAbsolute from 'path-absolute'
import * as R from 'ramda'
import renderHelp from 'render-help'
import * as installCommand from './install'
import getSaveType from './getSaveType'

const isWindows = process.platform === 'win32' || global['FAKE_WINDOWS']
const isFilespec = isWindows ? /^(?:[.]|~[/]|[/\\]|[a-zA-Z]:)/ : /^(?:[.]|~[/]|[/]|[a-zA-Z]:)/
const installLimit = pLimit(4)

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return R.pick([
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

  const [pkgPaths, pkgNames] = R.partition((inp) => isFilespec.test(inp), params)

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
  const { manifest, writeProjectManifest } = await readProjectManifest(cwd, opts)

  const newManifest = await link(pkgPaths, path.join(cwd, 'node_modules'), {
    ...linkOpts,
    manifest,
  })
  await writeProjectManifest(newManifest)

  await Promise.all(
    Array.from(storeControllerCache.values())
      .map(async (storeControllerPromise) => {
        const storeControllerHolder = await storeControllerPromise
        await storeControllerHolder.ctrl.close()
      })
  )
}
