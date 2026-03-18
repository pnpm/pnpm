import path from 'node:path'

import { OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import { docsUrl } from '@pnpm/cli.utils'
import { types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { existsNonEmptyWantedLockfile } from '@pnpm/lockfile.fs'
import { globalInfo } from '@pnpm/logger'
import type { Project } from '@pnpm/types'
import { findWorkspacePackages } from '@pnpm/workspace.projects-reader'
import { rimraf } from '@zkochan/rimraf'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import type { InstallCommandOptions } from './install.js'
import { installDeps } from './installDeps.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
    'child-concurrency',
    'dev',
    'engine-strict',
    'fetch-retries',
    'fetch-retry-factor',
    'fetch-retry-maxtimeout',
    'fetch-retry-mintimeout',
    'fetch-timeout',
    'global-pnpmfile',
    'hoist',
    'hoist-pattern',
    'https-proxy',
    'ignore-pnpmfile',
    'ignore-scripts',
    'modules-dir',
    'network-concurrency',
    'node-linker',
    'noproxy',
    'package-import-method',
    'pnpmfile',
    'prefer-offline',
    'production',
    'proxy',
    'public-hoist-pattern',
    'registry',
    'reporter',
    'shamefully-flatten',
    'shamefully-hoist',
    'shared-workspace-lockfile',
    'side-effects-cache-readonly',
    'side-effects-cache',
    'store-dir',
    'strict-peer-dependencies',
    'offline',
    'optional',
    'unsafe-perm',
    'verify-store-integrity',
    'virtual-store-dir',
  ], allTypes)
}

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  recursive: Boolean,
})

export const shorthands: Record<string, string> = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['ci', 'clean-install', 'ic', 'install-clean']

export function help (): string {
  return renderHelp({
    aliases: ['clean-install', 'ic', 'install-clean'],
    description: `Clean install a project. Removes node_modules and installs dependencies from the lockfile.
This command is similar to npm ci. It is designed for CI/CD environments and will fail if:
- The lockfile is missing
- The lockfile is not in sync with package.json`,
    descriptionLists: [
      {
        title: 'Options',
        list: [
          OPTIONS.ignoreScripts,
          OPTIONS.offline,
          OPTIONS.preferOffline,
          {
            description: "Packages in `devDependencies` won't be installed",
            name: '--prod',
            shortAlias: '-P',
          },
          {
            description: 'Only `devDependencies` are installed',
            name: '--dev',
            shortAlias: '-D',
          },
          {
            description: '`optionalDependencies` are not installed',
            name: '--no-optional',
          },
          OPTIONS.storeDir,
          OPTIONS.virtualStoreDir,
          ...UNIVERSAL_OPTIONS,
        ],
      },
    ],
    url: docsUrl('ci'),
    usages: [
      'pnpm ci',
      'pnpm ci --prod',
      'pnpm ci --ignore-scripts',
    ],
  })
}

export async function handler (opts: InstallCommandOptions): Promise<void> {
  const dir = opts.dir || process.cwd()
  const lockfileDir = opts.lockfileDir || dir
  const modulesDir = opts.modulesDir || 'node_modules'

  // 1. Check if lockfile exists
  const hasLockfile = await existsNonEmptyWantedLockfile(lockfileDir)

  if (!hasLockfile) {
    throw new PnpmError(
      'CI_LOCKFILE_MISSING',
      'Cannot perform a clean install because the lockfile is missing',
      {
        hint: 'Run "pnpm install" first to generate a lockfile, then commit it to your repository.',
      }
    )
  }

  // 2. Remove node_modules directories
  if (opts.workspaceDir) {
    // Workspace: remove node_modules from all projects
    const allProjects: Project[] = opts.allProjects ?? await findWorkspacePackages(opts.workspaceDir, {
      ...opts,
      patterns: opts.workspacePackagePatterns,
    })

    globalInfo(`Removing ${modulesDir} from ${allProjects.length} workspace projects...`)

    await Promise.all(
      allProjects.map(async (project) => {
        const projectModulesDir = path.join(project.rootDir, modulesDir)
        await rimraf(projectModulesDir)
      })
    )

    // Also remove root node_modules
    await rimraf(path.join(opts.workspaceDir, modulesDir))
  } else {
    // Single project: remove node_modules
    const projectModulesDir = path.join(dir, modulesDir)
    globalInfo(`Removing ${projectModulesDir}...`)
    await rimraf(projectModulesDir)
  }

  // 3. Install with frozen lockfile
  // Set frozenLockfile in opts before passing to installDeps
  // This ensures the lockfile won't be modified
  const ciOpts: InstallCommandOptions = {
    ...opts,
    frozenLockfile: true,
    preferFrozenLockfile: true,
  }

  const include = {
    dependencies: ciOpts.production !== false,
    devDependencies: ciOpts.dev !== false,
    optionalDependencies: ciOpts.optional !== false,
  }

  return installDeps(
    {
      ...ciOpts,
      allowNew: false,
      update: false,
      include,
      includeDirect: include,
    },
    []
  )
}
