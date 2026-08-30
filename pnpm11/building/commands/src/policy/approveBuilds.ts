import fs from 'node:fs'

import { checkbox, confirm } from '@inquirer/prompts'
import { allowBuildKeyFromIgnoredBuild } from '@pnpm/building.policy'
import type { CommandHandlerMap } from '@pnpm/cli.command'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { writeSettings } from '@pnpm/config.writer'
import { PnpmError } from '@pnpm/error'
import { scanGlobalPackages } from '@pnpm/global.packages'
import { install } from '@pnpm/installing.commands'
import { type Modules, writeModulesManifest } from '@pnpm/installing.modules-yaml'
import { globalInfo } from '@pnpm/logger'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import chalk from 'chalk'
import { isSubdir } from 'is-subdir'
import { renderHelp } from 'render-help'

import { rebuild, type RebuildCommandOpts } from '../build/index.js'
import { getAutomaticallyIgnoredBuilds } from './getAutomaticallyIgnoredBuilds.js'

export type ApproveBuildsCommandOpts = Pick<Config, 'modulesDir' | 'dir' | 'allowBuilds' | 'enableGlobalVirtualStore' | 'globalPkgDir'> & Pick<ConfigContext, 'rootProjectManifest' | 'rootProjectManifestDir'> & {
  all?: boolean
  global?: boolean
  /**
   * When set, overrides the target directory for writeSettings.
   * Used by the global-install flow to point allowBuilds updates at the
   * global pnpm-workspace.yaml while keeping workspaceDir unset so the
   * install itself targets only the single install directory.
   */
  settingsDir?: string
}

export const commandNames = ['approve-builds']

// pnpm-workspace.yaml settings that allowBuilds replaced in pnpm 11.
const LEGACY_BUILD_SETTINGS = ['onlyBuiltDependencies', 'onlyBuiltDependenciesFile', 'neverBuiltDependencies', 'ignoredBuiltDependencies']

export const recursiveByDefault = true

export function help (): string {
  return renderHelp({
    description: 'Approve dependencies for running scripts during installation',
    usages: [
      'pnpm approve-builds',
      'pnpm approve-builds [<pkg> ...] [!<pkg> ...]',
    ],
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Approve all pending dependencies without interactive prompts',
            name: '--all',
          },
          {
            description: 'Approve builds for globally installed packages',
            name: '--global',
            shortAlias: '-g',
          },
        ],
      },
    ],
  })
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    all: Boolean,
    global: Boolean,
  }
}

export function rcOptionsTypes (): Record<string, unknown> {
  return {}
}

export async function handler (opts: ApproveBuildsCommandOpts & RebuildCommandOpts, params: string[] = [], commands?: CommandHandlerMap): Promise<void> {
  if (opts.all && params.length) {
    throw new PnpmError(
      'APPROVE_BUILDS_ALL_WITH_ARGS',
      'Cannot use --all with positional arguments'
    )
  }
  const targets = await getApprovalTargets(opts)
  const automaticallyIgnoredBuilds = sortUniqueStrings(targets.flatMap((target) => target.automaticallyIgnoredBuilds ?? []))
  if (!automaticallyIgnoredBuilds.length) {
    globalInfo('There are no packages awaiting approval')
    return
  }
  const denied: string[] = []
  const approved: string[] = []
  const unknown: string[] = []
  for (const p of params) {
    const name = p.startsWith('!') ? p.slice(1) : p
    if (!automaticallyIgnoredBuilds.includes(name)) {
      unknown.push(name)
    } else if (p.startsWith('!')) {
      denied.push(name)
    } else {
      approved.push(name)
    }
  }
  if (unknown.length) {
    throw new PnpmError(
      'APPROVE_BUILDS_UNKNOWN_PACKAGES',
      `The following packages are not awaiting approval: ${unknown.join(', ')}`
    )
  }
  const contradictions = approved.filter((p) => denied.includes(p))
  if (contradictions.length) {
    throw new PnpmError(
      'APPROVE_BUILDS_CONTRADICTING_ARGS',
      `The following packages are both approved and denied: ${contradictions.join(', ')}`
    )
  }
  let buildPackages: string[] = []
  if (params.length) {
    buildPackages = sortUniqueStrings([...approved])
  } else if (opts.all) {
    buildPackages = sortUniqueStrings([...automaticallyIgnoredBuilds])
  } else {
    try {
      const buildPackagesValues = await checkbox({
        choices: sortUniqueStrings([...automaticallyIgnoredBuilds]).map((name) => ({
          name,
          value: name,
        })),
        message: 'Choose which packages to build ' +
          `(Press ${chalk.cyan('<space>')} to select, ` +
          `${chalk.cyan('<a>')} to toggle all, ` +
          `${chalk.cyan('<i>')} to invert selection)`,
        required: false,
        theme: {
          icon: { checked: '●', unchecked: '○', cursor: '❯' },
          style: {
            highlight: chalk.bgBlack.whiteBright,
          },
          keybindings: ['vim'],
        },
      })
      buildPackages = buildPackagesValues
    } catch (err) {
      if (err instanceof Error && err.name === 'ExitPromptError') {
        process.exit(0)
      }
      throw err
    }
  }
  const existingAllowBuilds = opts.global
    ? (await readWorkspaceManifest(opts.globalPkgDir))?.allowBuilds
    : opts.allowBuilds
  const allowBuilds: Record<string, boolean | string> = { ...existingAllowBuilds }
  if (params.length) {
    for (const pkg of approved) {
      allowBuilds[pkg] = true
    }
    for (const pkg of denied) {
      allowBuilds[pkg] = false
    }
  } else {
    const ignoredPackages = automaticallyIgnoredBuilds.filter((automaticallyIgnoredBuild) => !buildPackages.includes(automaticallyIgnoredBuild))
    for (const pkg of ignoredPackages) {
      allowBuilds[pkg] = false
    }
    for (const pkg of buildPackages) {
      allowBuilds[pkg] = true
    }
  }
  if (!opts.all && !params.length) {
    if (buildPackages.length) {
      let isConfirmed: boolean
      try {
        isConfirmed = await confirm({
          message: `The next packages will now be built: ${buildPackages.join(', ')}.\nDo you approve?`,
          default: false,
        })
      } catch (err) {
        if (err instanceof Error && err.name === 'ExitPromptError') {
          process.exit(0)
        }
        throw err
      }
      if (!isConfirmed) {
        return
      }
    } else {
      globalInfo('All packages were added to allowBuilds with value false.')
    }
  }
  await writeSettings({
    ...opts,
    workspaceDir: opts.settingsDir ?? (opts.global ? opts.globalPkgDir : opts.workspaceDir ?? opts.rootProjectManifestDir),
    updatedSettings: { allowBuilds },
    deletedLegacyKeys: LEGACY_BUILD_SETTINGS,
  })
  const decided = new Set([...approved, ...denied])
  await Promise.all(targets.map(async ({ modulesDir, modulesManifest }) => {
    if (!modulesManifest?.ignoredBuilds) return
    if (!params.length) {
      delete modulesManifest.ignoredBuilds
    } else {
      for (const depPath of modulesManifest.ignoredBuilds) {
        if (decided.has(allowBuildKeyFromIgnoredBuild(depPath))) {
          modulesManifest.ignoredBuilds.delete(depPath)
        }
      }
      if (!modulesManifest.ignoredBuilds.size) delete modulesManifest.ignoredBuilds
    }
    await writeModulesManifest(modulesDir, modulesManifest as Modules)
  }))
  await Promise.all(targets.map(async (target) => {
    const targetBuildPackages = buildPackages.filter((name) => target.automaticallyIgnoredBuilds?.includes(name))
    if (!targetBuildPackages.length) return
    if (target.opts.enableGlobalVirtualStore) {
      await install.handler({
        ...target.opts,
        allowBuilds,
        frozenLockfile: true,
        optimisticRepeatInstall: false,
      } as any, [], commands) // eslint-disable-line @typescript-eslint/no-explicit-any
      return
    }
    await rebuild.handler({
      ...target.opts,
      allowBuilds,
    }, targetBuildPackages)
  }))
}

interface ApprovalTarget {
  automaticallyIgnoredBuilds: string[] | null
  modulesDir: string
  modulesManifest: Modules | null
  opts: ApproveBuildsCommandOpts & RebuildCommandOpts
}

async function getApprovalTargets (opts: ApproveBuildsCommandOpts & RebuildCommandOpts): Promise<ApprovalTarget[]> {
  if (!opts.global) {
    return [{ ...await getAutomaticallyIgnoredBuilds(opts), opts }]
  }
  const scannedGroups = scanGlobalPackages(opts.globalPkgDir)
  if (!scannedGroups.length) return []
  const globalPkgDir = fs.realpathSync(opts.globalPkgDir)
  const groups = scannedGroups.filter(({ installDir }) => isSubdir(globalPkgDir, installDir))
  return Promise.all(groups.map(async ({ installDir }) => {
    const groupOpts = {
      ...opts,
      allProjects: undefined,
      dir: installDir,
      global: false,
      lockfileDir: installDir,
      modulesDir: undefined,
      rootProjectManifest: undefined,
      rootProjectManifestDir: installDir,
      selectedProjectsGraph: undefined,
      workspaceDir: undefined,
      workspacePackagePatterns: undefined,
    } as ApproveBuildsCommandOpts & RebuildCommandOpts
    return { ...await getAutomaticallyIgnoredBuilds(groupOpts), opts: groupOpts }
  }))
}

function sortUniqueStrings (array: string[]): string[] {
  return Array.from(new Set(array)).sort(lexCompare)
}
