import path from 'node:path'

import { UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import {
  docsUrl,
  tryReadProjectManifest,
} from '@pnpm/cli.utils'
import { type Config, types as allTypes } from '@pnpm/config.reader'
import { writeSettings } from '@pnpm/config.writer'
import { PnpmError } from '@pnpm/error'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/installing.context'
import type {
  WorkspacePackages,
} from '@pnpm/installing.deps-installer'
import { logger } from '@pnpm/logger'
import { DEPENDENCIES_FIELDS, type Project, type ProjectManifest } from '@pnpm/types'
import { findWorkspacePackages } from '@pnpm/workspace.projects-reader'
import normalize from 'normalize-path'
import { partition, pick } from 'ramda'
import { renderHelp } from 'render-help'

import { createProjectManifestWriter } from './createProjectManifestWriter.js'
import { getSaveType } from './getSaveType.js'
import * as install from './install.js'

// @ts-expect-error
const isWindows = process.platform === 'win32' || global['FAKE_WINDOWS']
const isFilespec = isWindows ? /^(?:[./\\]|~\/|[a-z]:)/i : /^(?:[./]|~\/|[a-z]:)/i

type LinkOpts = Pick<Config,
| 'bin'
| 'cliOptions'
| 'engineStrict'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
| 'overrides'
| 'saveDev'
| 'saveOptional'
| 'saveProd'
| 'workspaceDir'
| 'workspacePackagePatterns'
| 'sharedWorkspaceLockfile'
> & Partial<Pick<Config, 'linkWorkspacePackages'>> & install.InstallCommandOptions & {
  file?: boolean
  link?: boolean
}

type LocalProtocol = 'file:' | 'link:'

export const shorthands: Record<string, string> = {
  f: '--file',
  l: '--link',
}

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...pick([
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
    ], allTypes),
    file: Boolean,
    link: Boolean,
  }
}

export const commandNames = ['link', 'ln']

export function help (): string {
  return renderHelp({
    aliases: ['ln'],
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Use the `file:` protocol when linking local directories',
            name: '--file',
            shortAlias: '-f',
          },
          {
            description: 'Use the `link:` protocol when linking local directories (default)',
            name: '--link',
            shortAlias: '-l',
          },
          ...UNIVERSAL_OPTIONS,
        ],
      },
    ],
    url: docsUrl('link'),
    usages: [
      'pnpm link <dir>',
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
  if (opts.file && opts.link) {
    throw new PnpmError('LINK_PROTOCOL_OPTIONS_CONFLICT', '--file may not be used with --link')
  }

  const localProtocol = opts.file ? 'file:' : 'link:'

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

  const writeProjectManifest = await createProjectManifestWriter(opts.rootProjectManifestDir)

  if ((params == null) || (params.length === 0)) {
    throw new PnpmError('LINK_BAD_PARAMS', 'You must provide a parameter. Usage: pnpm link <dir>')
  }

  const [pkgPaths, pkgNames] = partition((inp) => isFilespec.test(inp), params)

  if (pkgNames.length > 0) {
    throw new PnpmError('LINK_BAD_PARAMS',
      `Cannot link by package name. Use a relative or absolute path instead, e.g. "pnpm link ./${pkgNames[0]}"`)
  }

  const newManifest = opts.rootProjectManifest ?? {}
  await Promise.all(
    pkgPaths.map(async (dir) => {
      await addLinkToManifest(opts, newManifest, dir, opts.rootProjectManifestDir, localProtocol)
      await checkPeerDeps(dir, opts)
    })
  )

  await writeProjectManifest(newManifest)
  await install.handler({
    ...linkOpts,
    _calledFromLink: true,
    frozenLockfileIfExists: false,
    rootProjectManifest: newManifest,
  })
}

async function addLinkToManifest (
  opts: LinkOpts,
  manifest: ProjectManifest,
  linkedDepDir: string,
  manifestDir: string,
  localProtocol: LocalProtocol
) {
  const { manifest: linkedManifest } = await tryReadProjectManifest(linkedDepDir, opts)
  const linkedPkgName = linkedManifest?.name ?? path.basename(linkedDepDir)
  const linkedPkgSpec = `${localProtocol}${normalize(path.relative(manifestDir, linkedDepDir))}`
  opts.overrides = {
    ...opts.overrides,
    [linkedPkgName]: linkedPkgSpec,
  }
  await writeSettings({
    ...opts,
    workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
    updatedSettings: {
      overrides: opts.overrides,
    },
  })
  if (DEPENDENCIES_FIELDS.every((depField) => manifest[depField]?.[linkedPkgName] == null)) {
    manifest.dependencies = manifest.dependencies ?? {}
    manifest.dependencies[linkedPkgName] = linkedPkgSpec
  }
}
