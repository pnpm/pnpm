import {
  docsUrl,
  getConfig,
  getSaveType,
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
  WorkspacePackages,
} from 'supi'
import * as installCommand from './install'
import recursive from './recursive'

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
  opts: CreateStoreControllerOptions & Pick<Config,
    'cliOptions' |
    'engineStrict' |
    'include' |
    'saveDev' |
    'saveOptional' |
    'saveProd' |
    'workspaceDir'
  > & Partial<Pick<Config, 'globalBin' | 'globalDir' | 'linkWorkspacePackages'>>,
) {
  const cwd = opts?.dir ?? process.cwd()

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
  if (!input || !input.length) {
    const { manifest, writeProjectManifest } = await tryReadProjectManifest(opts.globalDir!, opts)
    const newManifest = await linkToGlobal(cwd, {
      ...linkOpts,
      // A temporary workaround. global bin/prefix are always defined when --global is set
      globalBin: linkOpts.globalBin!,
      globalDir: linkOpts.globalDir!,
      manifest: manifest || {},
    })
    await writeProjectManifest(newManifest)
    return
  }

  const [pkgPaths, pkgNames] = R.partition((inp) => inp.startsWith('.'), input)

  if (pkgNames.length) {
    let globalPkgNames!: string[]
    if (opts.workspaceDir) {
      workspacePackagesArr = await findWorkspacePackages(opts.workspaceDir, opts)

      const pkgsFoundInWorkspace = workspacePackagesArr
        .filter(({ manifest }) => manifest.name && pkgNames.includes(manifest.name))
      pkgsFoundInWorkspace.forEach((pkgFromWorkspace) => pkgPaths.push(pkgFromWorkspace.dir))

      if (pkgsFoundInWorkspace.length && !linkOpts.targetDependenciesField) {
        linkOpts.targetDependenciesField = 'dependencies'
      }

      globalPkgNames = pkgNames.filter((pkgName) => !pkgsFoundInWorkspace.some((pkgFromWorkspace) => pkgFromWorkspace.manifest.name === pkgName))
    } else {
      globalPkgNames = pkgNames
    }
    const globalPkgPath = pathAbsolute(opts.globalDir!)
    globalPkgNames.forEach((pkgName) => pkgPaths.push(path.join(globalPkgPath, 'node_modules', pkgName)))
  }

  await Promise.all(
    pkgPaths.map((dir) => installLimit(async () => {
      const s = await createOrConnectStoreControllerCached(storeControllerCache, opts)
      await install(
        await readProjectManifestOnly(dir, opts), {
          ...await getConfig(
            { ...opts.cliOptions, 'dir': dir },
            {
              command: ['link'],
              excludeReporter: true,
              rcOptionsTypes: installCommand.rcOptionsTypes(),
              workspaceDir: await findWorkspaceDir(dir),
            },
          ),
          storeController: s.ctrl,
          storeDir: s.dir,
          workspacePackages,
        } as InstallOptions,
      )
    })),
  )
  const { manifest, writeProjectManifest } = await readProjectManifest(cwd, opts)

  // When running `pnpm link --production ../source`
  // only the `source` project should be pruned using the --production flag.
  // The target directory should keep its existing dependencies.
  // Except the ones that are replaced by the link.
  delete linkOpts.include

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
      }),
  )
}
