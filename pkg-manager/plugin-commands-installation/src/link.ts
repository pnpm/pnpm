import path from 'path'
import {
  docsUrl,
  getConfig,
  readProjectManifest,
  readProjectManifestOnly,
  tryReadProjectManifest,
} from '@pnpm/cli-utils'
import { UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { type Config, getOptionsFromRootManifest, types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import { findWorkspaceDir } from '@pnpm/find-workspace-dir'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/get-context'
import { findWorkspacePackages } from '@pnpm/workspace.find-packages'
import { type StoreController } from '@pnpm/package-store'
import { createOrConnectStoreControllerCached, type CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import {
  addDependenciesToPackage,
  install,
  type InstallOptions,
  link,
  type LinkFunctionOptions,
  type WorkspacePackages,
} from '@pnpm/core'
import { logger } from '@pnpm/logger'
import { type Project } from '@pnpm/types'
import pLimit from 'p-limit'
import pathAbsolute from 'path-absolute'
import pick from 'ramda/src/pick'
import partition from 'ramda/src/partition'
import renderHelp from 'render-help'
import * as installCommand from './install'
import { getSaveType } from './getSaveType'

// @ts-expect-error
const isWindows = process.platform === 'win32' || global['FAKE_WINDOWS']
const isFilespec = isWindows ? /^(?:[.]|~[/]|[/\\]|[a-zA-Z]:)/ : /^(?:[.]|~[/]|[/]|[a-zA-Z]:)/
const installLimit = pLimit(4)

type LinkOpts = CreateStoreControllerOptions & Pick<Config,
| 'bin'
| 'cliOptions'
| 'engineStrict'
| 'saveDev'
| 'saveOptional'
| 'saveProd'
| 'workspaceDir'
| 'workspacePackagePatterns'
| 'sharedWorkspaceLockfile'
> & Partial<Pick<Config, 'linkWorkspacePackages'>>

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
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

export function help (): string {
  return renderHelp({
    aliases: ['ln'],
    descriptionLists: [
      {
        title: 'Options',

        list: [
          ...UNIVERSAL_OPTIONS,
          {
            description: 'Link package to/from global node_modules',
            name: '--global',
            shortAlias: '-g',
          },
        ],
      },
    ],
    url: docsUrl('link'),
    usages: [
      'pnpm link <dir>',
      'pnpm link --global (in package dir)',
      'pnpm link --global <pkg>',
    ],
  })
}

async function checkPeerDeps (linkCwdDir: string, opts: LinkOpts) {
  const { manifest } = await tryReadProjectManifest(linkCwdDir, opts)

  if (manifest?.peerDependencies && Object.keys(manifest.peerDependencies).length > 0) {
    const packageName = manifest.name ?? path.basename(linkCwdDir) // Assuming the name property exists in newManifest
    const peerDeps = Object.entries(manifest.peerDependencies)
      .map(([key, value]) => `  - ${key}@${value}`)
      .join(', ')

    logger.warn({
      message: `The package ${packageName}, which you have just pnpm linked, has the following peerDependencies specified in its package.json:

${peerDeps}

The linked in dependency will not resolve the peer dependencies from the target node_modules.
This might cause issues in your project. To resolve this, you may use the "file:" protocol to reference the local dependency.`,
      prefix: opts.dir,
    })
  }
}

export async function handler (
  opts: LinkOpts,
  params?: string[]
): Promise<void> {
  const cwd = process.cwd()

  const storeControllerCache = new Map<string, Promise<{ dir: string, ctrl: StoreController }>>()
  let workspacePackagesArr: Project[]
  let workspacePackages!: WorkspacePackages
  if (opts.workspaceDir) {
    workspacePackagesArr = await findWorkspacePackages(opts.workspaceDir, {
      ...opts,
      patterns: opts.workspacePackagePatterns,
    })
    workspacePackages = arrayOfWorkspacePackagesToMap(workspacePackagesArr) as WorkspacePackages
  } else {
    workspacePackages = new Map()
  }

  const store = await createOrConnectStoreControllerCached(storeControllerCache, opts)
  const linkOpts = Object.assign(opts, {
    storeController: store.ctrl,
    storeDir: store.dir,
    targetDependenciesField: getSaveType(opts),
    workspacePackages,
  })

  const linkCwdDir = opts.cliOptions?.dir && opts.cliOptions?.global ? path.resolve(opts.cliOptions.dir) : cwd

  // pnpm link
  if ((params == null) || (params.length === 0)) {
    if (path.relative(linkOpts.dir, cwd) === '') {
      throw new PnpmError('LINK_BAD_PARAMS', 'You must provide a parameter')
    }

    await checkPeerDeps(linkCwdDir, opts)

    const { manifest, writeProjectManifest } = await tryReadProjectManifest(opts.dir, opts)
    const newManifest = await addDependenciesToPackage(
      manifest ?? {},
      [`link:${linkCwdDir}`],
      linkOpts
    )
    await writeProjectManifest(newManifest)
    return
  }

  const [pkgPaths, pkgNames] = partition((inp) => isFilespec.test(inp), params)

  await Promise.all(
    pkgPaths.map(async (dir) => installLimit(async () => {
      const s = await createOrConnectStoreControllerCached(storeControllerCache, opts)
      const config = await getConfig(
        { ...opts.cliOptions, dir },
        {
          excludeReporter: true,
          rcOptionsTypes: installCommand.rcOptionsTypes(),
          workspaceDir: await findWorkspaceDir(dir),
        }
      )
      await install(
        await readProjectManifestOnly(dir, opts), {
          ...config,
          ...getOptionsFromRootManifest(config.rootProjectManifestDir, config.rootProjectManifest ?? {}),
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
      workspacePackagesArr = await findWorkspacePackages(opts.workspaceDir, {
        ...opts,
        patterns: opts.workspacePackagePatterns,
      })

      const pkgsFoundInWorkspace = workspacePackagesArr
        .filter(({ manifest }) => manifest.name && pkgNames.includes(manifest.name))
      pkgsFoundInWorkspace.forEach((pkgFromWorkspace) => pkgPaths.push(pkgFromWorkspace.rootDir))

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

  const { manifest, writeProjectManifest } = await readProjectManifest(linkCwdDir, opts)

  await Promise.all(
    pkgPaths.map(async (dir) => {
      await checkPeerDeps(dir, opts)
    })
  )

  const linkConfig = await getConfig(
    { ...opts.cliOptions, dir: cwd },
    {
      excludeReporter: true,
      rcOptionsTypes: installCommand.rcOptionsTypes(),
      workspaceDir: await findWorkspaceDir(cwd),
    }
  )
  const storeL = await createOrConnectStoreControllerCached(storeControllerCache, linkConfig)
  const newManifest = await link(pkgPaths, path.join(linkCwdDir, 'node_modules'), {
    ...linkConfig,
    targetDependenciesField: linkOpts.targetDependenciesField,
    storeController: storeL.ctrl,
    storeDir: storeL.dir,
    manifest,
  } as LinkFunctionOptions)
  if (!opts.cliOptions?.global) {
    await writeProjectManifest(newManifest)
  }

  await Promise.all(
    Array.from(storeControllerCache.values())
      .map(async (storeControllerPromise) => {
        const storeControllerHolder = await storeControllerPromise
        await storeControllerHolder.ctrl.close()
      })
  )
}
