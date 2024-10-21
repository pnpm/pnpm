import path from 'path'
import {
  docsUrl,
  readProjectManifest,
  tryReadProjectManifest,
  type ReadProjectManifestOpts,
} from '@pnpm/cli-utils'
import { UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { type Config, types as allTypes } from '@pnpm/config'
import { DEPENDENCIES_FIELDS, type ProjectManifest, type Project } from '@pnpm/types'
import { PnpmError } from '@pnpm/error'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/get-context'
import { findWorkspacePackages } from '@pnpm/workspace.find-packages'
import { type StoreController } from '@pnpm/package-store'
import { createOrConnectStoreControllerCached, type CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import {
  install,
  type WorkspacePackages,
} from '@pnpm/core'
import { logger } from '@pnpm/logger'
import pick from 'ramda/src/pick'
import partition from 'ramda/src/partition'
import renderHelp from 'render-help'
import { getSaveType } from './getSaveType'

// @ts-expect-error
const isWindows = process.platform === 'win32' || global['FAKE_WINDOWS']
const isFilespec = isWindows ? /^(?:[.]|~[/]|[/\\]|[a-zA-Z]:)/ : /^(?:[.]|~[/]|[/]|[a-zA-Z]:)/

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
| 'globalDirPrefix'
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
    binsDir: opts.bin,
  })

  // "pnpm link"
  if ((params == null) || (params.length === 0)) {
    if (path.relative(linkOpts.dir, cwd) === '') {
      throw new PnpmError('LINK_BAD_PARAMS', 'You must provide a parameter')
    }

    await checkPeerDeps(cwd, opts)

    const { manifest, writeProjectManifest } = await tryReadProjectManifest(linkOpts.globalDirPrefix, opts)
    const newManifest = manifest ?? {}
    await addLinkToManifest(opts, newManifest, cwd, linkOpts.globalDirPrefix)
    await writeProjectManifest(newManifest)
    await install(newManifest, linkOpts)
    return
  }

  const [pkgPaths, pkgNames] = partition((inp) => isFilespec.test(inp), params)

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
    globalPkgNames.forEach((pkgName) => pkgPaths.push(path.join(opts.globalDirPrefix, 'node_modules', pkgName)))
  }

  const { manifest, writeProjectManifest } = await readProjectManifest(opts.dir, opts)

  const newManifest = manifest ?? {}
  await Promise.all(
    pkgPaths.map(async (dir) => {
      await addLinkToManifest(opts, newManifest, dir, opts.dir)
      await checkPeerDeps(dir, opts)
    })
  )

  await writeProjectManifest(newManifest)
  await install(newManifest, linkOpts)
}

async function addLinkToManifest (opts: ReadProjectManifestOpts, manifest: ProjectManifest, linkedDepDir: string, dependentDir: string) {
  if (!manifest.pnpm) {
    manifest.pnpm = {
      overrides: {},
    }
  } else if (!manifest.pnpm.overrides) {
    manifest.pnpm.overrides = {}
  }
  const { manifest: linkedManifest } = await tryReadProjectManifest(linkedDepDir, opts)
  const linkedPkgName = linkedManifest?.name ?? path.basename(linkedDepDir)
  const linkedPkgSpec = `link:${path.relative(dependentDir, linkedDepDir)}`
  manifest.pnpm.overrides![linkedPkgName] = linkedPkgSpec
  if (DEPENDENCIES_FIELDS.every((depField) => manifest[depField]?.[linkedPkgName] == null)) {
    manifest.dependencies = manifest.dependencies ?? {}
    manifest.dependencies[linkedPkgName] = linkedPkgSpec
  }
}
