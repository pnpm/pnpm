import path from 'path'
import {
  docsUrl,
  tryReadProjectManifest,
  type ReadProjectManifestOpts,
} from '@pnpm/cli-utils'
import { UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { type Config, types as allTypes } from '@pnpm/config'
import { DEPENDENCIES_FIELDS, type ProjectManifest, type Project } from '@pnpm/types'
import { PnpmError } from '@pnpm/error'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/get-context'
import { findWorkspacePackages } from '@pnpm/workspace.find-packages'
import {
  type WorkspacePackages,
} from '@pnpm/core'
import { logger } from '@pnpm/logger'
import pick from 'ramda/src/pick'
import partition from 'ramda/src/partition'
import renderHelp from 'render-help'
import { createProjectManifestWriter } from './createProjectManifestWriter'
import { getSaveType } from './getSaveType'
import * as install from './install'

// @ts-expect-error
const isWindows = process.platform === 'win32' || global['FAKE_WINDOWS']
const isFilespec = isWindows ? /^(?:[.]|~[/]|[/\\]|[a-zA-Z]:)/ : /^(?:[.]|~[/]|[/]|[a-zA-Z]:)/

type LinkOpts = Pick<Config,
| 'bin'
| 'cliOptions'
| 'engineStrict'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
| 'saveDev'
| 'saveOptional'
| 'saveProd'
| 'workspaceDir'
| 'workspacePackagePatterns'
| 'sharedWorkspaceLockfile'
| 'globalPkgDir'
> & Partial<Pick<Config, 'linkWorkspacePackages'>> & install.InstallCommandOptions

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

        list: UNIVERSAL_OPTIONS,
      },
    ],
    url: docsUrl('link'),
    usages: [
      'pnpm link <dir|pkg name>',
      'pnpm link',
    ],
  })
}

async function checkPeerDeps (linkCwdDir: string, opts: LinkOpts) {
  const { manifest } = await tryReadProjectManifest(linkCwdDir, opts)

  if (manifest?.peerDependencies && Object.keys(manifest.peerDependencies).length > 0) {
    const packageName = manifest.name ?? path.basename(linkCwdDir) // Assuming the name property exists in newManifest
    const peerDeps = Object.entries(manifest.peerDependencies)
      .map(([key, value]) => `  - ${key}@${String(value)}`)
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

  const linkOpts = Object.assign(opts, {
    targetDependenciesField: getSaveType(opts),
    workspacePackages,
    binsDir: opts.bin,
  })

  if (opts.cliOptions?.global && !opts.bin) {
    throw new PnpmError('NO_GLOBAL_BIN_DIR', 'Unable to find the global bin directory', {
      hint: 'Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable. The global bin directory should be in the PATH.',
    })
  }

  const writeProjectManifest = await createProjectManifestWriter(opts.rootProjectManifestDir)

  // pnpm link
  if ((params == null) || (params.length === 0)) {
    const cwd = process.cwd()
    if (path.relative(linkOpts.dir, cwd) === '') {
      throw new PnpmError('LINK_BAD_PARAMS', 'You must provide a parameter')
    }

    await checkPeerDeps(cwd, opts)

    const newManifest = opts.rootProjectManifest ?? {}
    await addLinkToManifest(opts, newManifest, cwd, opts.rootProjectManifestDir)
    await writeProjectManifest(newManifest)
    await install.handler({
      ...linkOpts,
      frozenLockfileIfExists: false,
      rootProjectManifest: newManifest,
    })
    return
  }

  const [pkgPaths, pkgNames] = partition((inp) => isFilespec.test(inp), params)

  pkgNames.forEach((pkgName) => pkgPaths.push(path.join(opts.globalPkgDir, 'node_modules', pkgName)))

  const newManifest = opts.rootProjectManifest ?? {}
  await Promise.all(
    pkgPaths.map(async (dir) => {
      await addLinkToManifest(opts, newManifest, dir, opts.rootProjectManifestDir)
      await checkPeerDeps(dir, opts)
    })
  )

  await writeProjectManifest(newManifest)
  await install.handler({
    ...linkOpts,
    frozenLockfileIfExists: false,
    rootProjectManifest: newManifest,
  })
}

async function addLinkToManifest (opts: ReadProjectManifestOpts, manifest: ProjectManifest, linkedDepDir: string, manifestDir: string) {
  if (!manifest.pnpm) {
    manifest.pnpm = {
      overrides: {},
    }
  } else if (!manifest.pnpm.overrides) {
    manifest.pnpm.overrides = {}
  }
  const { manifest: linkedManifest } = await tryReadProjectManifest(linkedDepDir, opts)
  const linkedPkgName = linkedManifest?.name ?? path.basename(linkedDepDir)
  const linkedPkgSpec = `link:${path.relative(manifestDir, linkedDepDir)}`
  manifest.pnpm.overrides![linkedPkgName] = linkedPkgSpec
  if (DEPENDENCIES_FIELDS.every((depField) => manifest[depField]?.[linkedPkgName] == null)) {
    manifest.dependencies = manifest.dependencies ?? {}
    manifest.dependencies[linkedPkgName] = linkedPkgSpec
  }
}
