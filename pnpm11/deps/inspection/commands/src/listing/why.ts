import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import { docsUrl } from '@pnpm/cli.utils'
import { types as allTypes } from '@pnpm/config.reader'
import {
  type DependentsTree,
  getDependentsTrees,
  mergeDependentsTrees,
  renderDependents,
  whyForPackages,
  type WhyOptions,
} from '@pnpm/deps.inspection.list'
import { PnpmError } from '@pnpm/error'
import { findGlobalInstallDirs } from '@pnpm/global.commands'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { BASE_RC_OPTION_KEYS, computeInclude, determineReportAs, resolveFinders, SHARED_CLI_HELP_OPTIONS } from './common.js'
import { EXCLUDE_PEERS_HELP, type ListCommandOptions } from './list.js'
import { whyRecursive } from './recursive.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([...BASE_RC_OPTION_KEYS, 'depth'], allTypes)
}

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  'exclude-peers': Boolean,
  recursive: Boolean,
  'find-by': [String, Array],
})

export { shorthands } from './common.js'

export const commandNames = ['why']

export const recursiveByDefault = true

export function help (): string {
  return renderHelp({
    description: `Shows the packages that depend on <pkg>
For example: pnpm why babel-* eslint-*`,
    descriptionLists: [
      {
        title: 'Options',

        list: [
          ...SHARED_CLI_HELP_OPTIONS,
          {
            description: 'Max display depth of the reverse dependency tree',
            name: '--depth <number>',
          },
          EXCLUDE_PEERS_HELP,
          OPTIONS.globalDir,
          ...UNIVERSAL_OPTIONS,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('why'),
    usages: [
      'pnpm why <pkg> ...',
    ],
  })
}

export async function handler (
  opts: ListCommandOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0 && opts.findBy == null) {
    throw new PnpmError('MISSING_PACKAGE_NAME', '`pnpm why` requires the package name or --find-by=<finder-name>')
  }

  const include = computeInclude(opts)
  const finders = resolveFinders(opts)
  const reportAs = determineReportAs(opts)
  const depth = opts.cliOptions?.['depth'] as number | undefined
  const whyOpts = {
    depth,
    include,
    long: opts.long,
    reportAs,
    modulesDir: opts.modulesDir,
    nodeLinker: opts.nodeLinker,
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
    checkWantedLockfileOnly: opts.lockfileOnly,
    excludePeerDependencies: opts.excludePeers,
    finders,
  }

  if (opts.global) {
    if (!opts.globalPkgDir) {
      throw new PnpmError('NO_GLOBAL_BIN_DIR', 'Unable to find the global packages directory')
    }
    return whyGlobal(opts.globalPkgDir, params, whyOpts)
  }

  if (opts.recursive && opts.selectedProjectsGraph != null) {
    const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)
    return whyRecursive(pkgs, params, {
      ...whyOpts,
      lockfileDir: opts.lockfileDir,
      packageConfigs: opts.packageConfigs,
    })
  }

  return whyForPackages(params, [opts.dir], {
    ...whyOpts,
    lockfileDir: opts.lockfileDir ?? opts.dir,
  })
}

async function whyGlobal (
  globalPkgDir: string,
  params: string[],
  whyOpts: Omit<WhyOptions, 'lockfileDir'>
): Promise<string> {
  const installDirs = findGlobalInstallDirs(globalPkgDir, [])
  const projectTrees = (await Promise.all(installDirs.map((installDir) =>
    getDependentsTrees(params, [installDir], {
      ...whyOpts,
      lockfileDir: installDir,
      modulesDir: undefined,
    })
  ))).filter((trees): trees is DependentsTree[] => trees != null)
  if (projectTrees.length === 0) return ''
  return renderDependents(mergeDependentsTrees(projectTrees.flat()), whyOpts)
}
