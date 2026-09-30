import type { CommandHandlerMap, CompletionFunc } from '@pnpm/cli.command'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import { docsUrl, readDepNameCompletions } from '@pnpm/cli.utils'
import { types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { handleGlobalUpdate, migrateLegacyGlobalPackages, selectsPnpmCli } from '@pnpm/global.commands'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { createGlobalPolicyCallbacks } from '../resolutionPolicyManifest.js'
import { interactiveUpdate } from './interactiveUpdate.js'
import { selectGlobalPackageGroups } from './selectGlobalPackageGroups.js'
import { assertPatchesOptions, update, type UpdateCommandOptions } from './update.js'

export type { UpdateCommandOptions } from './update.js'

const RC_OPTION_NAMES = [
  'cache-dir',
  'dangerously-allow-all-builds',
  'depth',
  'dev',
  'engine-strict',
  'fetch-retries',
  'fetch-retry-factor',
  'fetch-retry-maxtimeout',
  'fetch-retry-mintimeout',
  'fetch-timeout',
  'force',
  'global-dir',
  'global-pnpmfile',
  'global',
  'https-proxy',
  'ignore-pnpmfile',
  'ignore-scripts',
  'lockfile-dir',
  'lockfile-only',
  'lockfile',
  'lockfile-include-tarball-url',
  'network-concurrency',
  'node-experimental-package-map',
  'node-package-map-type',
  'noproxy',
  'npm-path',
  'offline',
  'only',
  'optional',
  'package-import-method',
  'pnpmfile',
  'prefer-offline',
  'production',
  'proxy',
  'registry',
  'reporter',
  'save',
  'save-exact',
  'save-prefix',
  'save-workspace-protocol',
  'scripts-prepend-node-path',
  'shamefully-hoist',
  'shared-workspace-lockfile',
  'side-effects-cache-readonly',
  'side-effects-cache',
  'store-dir',
  'trust-lockfile',
  'trust-policy',
  'trust-policy-exclude',
  'trust-policy-ignore-after',
  'unsafe-perm',
] as const

export function rcOptionsTypes (): Record<string, unknown> {
  return pick(RC_OPTION_NAMES, allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    changeset: Boolean,
    'include-github-actions': Boolean,
    interactive: Boolean,
    latest: Boolean,
    patches: Boolean,
    peer: Boolean,
    recursive: Boolean,
    workspace: Boolean,
  }
}

export const shorthands: Record<string, string> = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['update', 'up', 'upgrade']

export const completion: CompletionFunc = async (cliOpts) => {
  return readDepNameCompletions(cliOpts.dir as string)
}

const OPTIONS_HELP_LIST = [
  {
    description: 'Update in every package found in subdirectories \
or every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
    name: '--recursive',
    shortAlias: '-r',
  },
  {
    description: 'Update globally installed packages',
    name: '--global',
    shortAlias: '-g',
  },
  {
    description: 'How deep should levels of dependencies be inspected. Infinity is default. 0 would mean top-level dependencies only',
    name: '--depth <number>',
  },
  {
    description: 'Ignore version ranges in package.json',
    name: '--latest',
    shortAlias: '-L',
  },
  {
    description: 'Refresh registry revisions without changing package versions',
    name: '--patches',
  },
  {
    description: 'Update packages only in "dependencies" and "optionalDependencies"',
    name: '--prod',
    shortAlias: '-P',
  },
  {
    description: 'Update packages only in "devDependencies"',
    name: '--dev',
    shortAlias: '-D',
  },
  {
    description: 'Don\'t update packages in "optionalDependencies"',
    name: '--no-optional',
  },
  {
    description: 'Also update packages in "peerDependencies"',
    name: '--peer',
  },
  {
    description: 'Tries to link all packages from the workspace. \
Versions are updated to match the versions of packages inside the workspace. \
If specific packages are updated, the command will fail if any of the updated \
dependencies is not found inside the workspace',
    name: '--workspace',
  },
  {
    description: 'Show outdated dependencies and select which ones to update',
    name: '--interactive',
    shortAlias: '-i',
  },
  {
    description: 'Generate a changeset file declaring a patch bump for every workspace package whose production dependencies were changed by the update',
    name: '--changeset',
  },
  {
    description: 'Also update GitHub Actions dependencies in workflow and action files',
    name: '--include-github-actions',
  },
  {
    description: 'Don\'t update the ranges in package.json.',
    name: '--no-save',
  },
  OPTIONS.globalDir,
  ...UNIVERSAL_OPTIONS,
]

export function help (): string {
  return renderHelp({
    aliases: ['up', 'upgrade'],
    description: 'Updates package dependencies to their latest version based on the specified range. GitHub Actions dependencies can be included with --include-github-actions. You can use "*" in a dependency name to update all dependencies with the same pattern.',
    descriptionLists: [
      {
        title: 'Options',

        list: OPTIONS_HELP_LIST,
      },
      FILTERING,
    ],
    url: docsUrl('update'),
    usages: ['pnpm update [-g] [<pkg>...]'],
  })
}

export async function handler (
  opts: UpdateCommandOptions,
  params: string[] = [],
  commands?: CommandHandlerMap
): Promise<string | undefined> {
  assertPatchesOptions(params, opts)
  if (opts.global) {
    return updateGlobalPackages(params, opts, commands)
  }
  const rebuildHandler = commands?.rebuild
  if (opts.interactive) {
    return interactiveUpdate(params, opts, rebuildHandler)
  }
  return update(params, opts, rebuildHandler) as Promise<undefined>
}

async function updateGlobalPackages (
  params: string[],
  opts: UpdateCommandOptions,
  commands?: CommandHandlerMap
): Promise<string | undefined> {
  if (!opts.bin) {
    throw new PnpmError('NO_GLOBAL_BIN_DIR', 'Unable to find the global bin directory', {
      hint: 'Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable. The global bin directory should be in the PATH.',
    })
  }
  if (selectsPnpmCli(params)) {
    throw new PnpmError('GLOBAL_PNPM_INSTALL', 'Use the "pnpm self-update" command to install or update pnpm')
  }
  // Before the interactive selection, so the migrated groups are offered too.
  await migrateLegacyGlobalPackages({ ...opts, ...createGlobalPolicyCallbacks(opts) }, commands ?? {})
  const selection = opts.interactive
    ? await selectGlobalPackageGroups(params, opts)
    : undefined
  if (typeof selection === 'string') return selection
  if (selection?.size === 0) return undefined
  return handleGlobalUpdate({
    ...opts,
    ...createGlobalPolicyCallbacks(opts),
    selectedPackageHashes: selection,
  }, params, commands ?? {})
}
