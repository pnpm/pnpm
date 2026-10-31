import path from 'node:path'

import { UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import {
  docsUrl,
  tryReadProjectManifest,
} from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { writeSettings } from '@pnpm/config.writer'
import { PnpmError } from '@pnpm/error'
import { arrayOfWorkspacePackagesToMap } from '@pnpm/installing.context'
import type {
  WorkspacePackages,
} from '@pnpm/installing.deps-installer'
import { isValidDependencyAlias } from '@pnpm/installing.deps-resolver'
import { DEPENDENCIES_FIELDS, type Project, type ProjectManifest } from '@pnpm/types'
import { findWorkspaceProjects } from '@pnpm/workspace.projects-reader'
import normalize from 'normalize-path'
import { partition, pick } from 'ramda'
import { renderHelp } from 'render-help'

import { createProjectManifestWriter } from './createProjectManifestWriter.js'
import { getSaveType } from './getSaveType.js'
import * as install from './install.js'
import { warnAboutLinkedPeerDependencies } from './warnAboutLinkedPeerDependencies.js'

// @ts-expect-error -- FAKE_WINDOWS is a test-only global that is not declared on globalThis
const isWindows = process.platform === 'win32' || global['FAKE_WINDOWS']
const isFilespec = isWindows ? /^(?:[./\\]|~\/|[a-z]:)/i : /^(?:[./]|~\/|[a-z]:)/i

type LinkOpts = Pick<Config,
| 'bin'
| 'engineStrict'
| 'overrides'
| 'saveDev'
| 'saveOptional'
| 'saveProd'
| 'workspaceDir'
| 'workspacePackagePatterns'
| 'sharedWorkspaceLockfile'
> & Pick<ConfigContext,
| 'cliOptions'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
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
    'trust-lockfile',
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
      'pnpm link <dir>',
    ],
  })
}

async function checkPeerDeps (linkCwdDir: string, opts: LinkOpts) {
  const { manifest } = await tryReadProjectManifest(linkCwdDir, opts)
  warnAboutLinkedPeerDependencies(manifest, { pkgDir: linkCwdDir, prefix: opts.dir })
}

export async function handler (
  opts: LinkOpts,
  params?: string[]
): Promise<void> {
  if ((params == null) || (params.length === 0)) {
    throw new PnpmError('LINK_BAD_PARAMS', 'You must provide a parameter. Usage: pnpm link <dir>')
  }
  let workspacePackagesArr: Project[]
  let workspacePackages!: WorkspacePackages
  if (opts.workspaceDir) {
    workspacePackagesArr = await findWorkspaceProjects(opts.workspaceDir, {
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


  const [pkgPaths, pkgNames] = partition((inp) => isFilespec.test(inp), params)

  if (pkgNames.length > 0) {
    throw new PnpmError('LINK_BAD_PARAMS',
      `Cannot link by package name. Use a relative or absolute path instead, e.g. "pnpm link ./${pkgNames[0]}"`)
  }

  const newManifest = opts.rootProjectManifest ?? {}
  await addLinksToManifest(opts, newManifest, pkgPaths)
  await writeProjectManifest(newManifest)
  await install.handler({
    ...linkOpts,
    _calledFromLink: true,
    frozenLockfileIfExists: false,
    rootProjectManifest: newManifest,
  })
}

async function addLinksToManifest (opts: LinkOpts, manifest: ProjectManifest, pkgPaths: string[]): Promise<void> {
  // Every name is checked before the first link is written.
  const linkedPkgs = await Promise.all(
    pkgPaths.map(async (dir) => ({ dir, name: await readLinkedPkgName(dir, opts) }))
  )
  await Promise.all(
    linkedPkgs.map(async (linkedPkg) => {
      await addLinkToManifest(opts, manifest, linkedPkg, opts.rootProjectManifestDir)
      await checkPeerDeps(linkedPkg.dir, opts)
    })
  )
}

async function readLinkedPkgName (linkedDepDir: string, opts: LinkOpts): Promise<string> {
  const { manifest } = await tryReadProjectManifest(linkedDepDir, opts)
  const name = manifest?.name ?? path.basename(linkedDepDir)
  if (!isValidDependencyAlias(name)) {
    throw new PnpmError('INVALID_DEPENDENCY_NAME', `Cannot link "${linkedDepDir}" under the invalid package name ${JSON.stringify(name)}`, {
      hint: 'A package is linked under the "name" field of its package.json, or under its directory name if it has no "name" field. That name must be a valid npm package name.',
    })
  }
  return name
}

async function addLinkToManifest (
  opts: LinkOpts,
  manifest: ProjectManifest,
  { dir: linkedDepDir, name: linkedPkgName }: { dir: string, name: string },
  manifestDir: string
) {
  const linkedPkgSpec = `link:${normalize(path.relative(manifestDir, linkedDepDir))}`
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
