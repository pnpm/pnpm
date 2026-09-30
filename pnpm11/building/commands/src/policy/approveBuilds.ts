import fs from 'node:fs'

import { checkbox, confirm } from '@inquirer/prompts'
import { allowBuildKeyFromIgnoredBuild, parseAllowBuildSelector } from '@pnpm/building.policy'
import type { CommandHandlerMap } from '@pnpm/cli.command'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { writeSettings } from '@pnpm/config.writer'
import { isError, PnpmError } from '@pnpm/error'
import { scanGlobalPackages } from '@pnpm/global.packages'
import { install } from '@pnpm/installing.commands'
import { type Modules, writeModulesManifest } from '@pnpm/installing.modules-yaml'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { lexCompare } from '@pnpm/text.ordinal-comparator'
import type { IgnoredBuilds } from '@pnpm/types'
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
  validateParams(opts, params)
  const targets = await getApprovalTargets(opts)
  const automaticallyIgnoredBuilds = sortUniqueStrings(targets.flatMap((target) => target.automaticallyIgnoredBuilds ?? []))
  if (!automaticallyIgnoredBuilds.length && !params.length) {
    globalInfo('There are no packages awaiting approval')
    return
  }
  const { approved, denied } = parseSelectors(params, automaticallyIgnoredBuilds)
  const buildPackages = await chooseBuildPackages({ opts, params, approved, automaticallyIgnoredBuilds })
  const allowBuilds: Record<string, boolean | string> = { ...await readExistingAllowBuilds(opts) }
  if (params.length) {
    setAllowBuilds(allowBuilds, approved, true)
    setAllowBuilds(allowBuilds, denied, false)
  } else {
    setAllowBuilds(allowBuilds, automaticallyIgnoredBuilds.filter((automaticallyIgnoredBuild) => !buildPackages.includes(automaticallyIgnoredBuild)), false)
    setAllowBuilds(allowBuilds, buildPackages, true)
  }
  if (!opts.all && !params.length && !await confirmBuildPackages(buildPackages)) {
    return
  }
  await writeSettings({
    ...opts,
    workspaceDir: opts.settingsDir ?? (opts.global ? opts.globalPkgDir : opts.workspaceDir ?? opts.rootProjectManifestDir),
    updatedSettings: { allowBuilds },
    deletedLegacyKeys: LEGACY_BUILD_SETTINGS,
  })
  await clearDecidedIgnoredBuilds(targets, params.length ? new Set([...approved, ...denied]) : undefined)
  await buildApprovedPackages({ targets, buildPackages, allowBuilds, commands })
}

function validateParams (opts: ApproveBuildsCommandOpts, params: string[]): void {
  if (opts.all && params.length) {
    throw new PnpmError(
      'APPROVE_BUILDS_ALL_WITH_ARGS',
      'Cannot use --all with positional arguments'
    )
  }
  if (params.some((param) => parseAllowBuildSelector(param).name === '')) {
    throw new PnpmError(
      'APPROVE_BUILDS_MISSING_PACKAGE',
      'A package name is missing from the arguments. Please specify the package name(s) to approve (`<pkg>`) or deny (`!<pkg>`).'
    )
  }
}

interface ParsedSelectors {
  approved: string[]
  denied: string[]
}

function parseSelectors (params: string[], automaticallyIgnoredBuilds: string[]): ParsedSelectors {
  const denied: string[] = []
  const approved: string[] = []
  const unknown: string[] = []
  for (const selector of params) {
    const { name, allowed } = parseAllowBuildSelector(selector)
    if (!automaticallyIgnoredBuilds.includes(name)) {
      unknown.push(name)
    }
    if (allowed) {
      approved.push(name)
    } else {
      denied.push(name)
    }
  }
  if (unknown.length) {
    globalWarn(`The following packages are not awaiting approval: ${unknown.join(', ')}`)
  }
  const contradictions = approved.filter((name) => denied.includes(name))
  if (contradictions.length) {
    throw new PnpmError(
      'APPROVE_BUILDS_CONTRADICTING_ARGS',
      `The following packages are both approved and denied: ${contradictions.join(', ')}`
    )
  }
  return { approved, denied }
}

interface ChooseBuildPackagesOptions {
  opts: ApproveBuildsCommandOpts
  params: string[]
  approved: string[]
  automaticallyIgnoredBuilds: string[]
}

async function chooseBuildPackages ({ opts, params, approved, automaticallyIgnoredBuilds }: ChooseBuildPackagesOptions): Promise<string[]> {
  if (params.length) {
    return sortUniqueStrings([...approved])
  }
  if (opts.all) {
    return sortUniqueStrings([...automaticallyIgnoredBuilds])
  }
  return exitOnPromptCancel(() => checkbox({
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
  }))
}

async function readExistingAllowBuilds (opts: ApproveBuildsCommandOpts): Promise<Config['allowBuilds'] | undefined> {
  return opts.global
    ? (await readWorkspaceManifest(opts.globalPkgDir))?.allowBuilds
    : opts.allowBuilds
}

function setAllowBuilds (allowBuilds: Record<string, boolean | string>, pkgs: string[], value: boolean): void {
  for (const pkg of pkgs) {
    allowBuilds[pkg] = value
  }
}

async function confirmBuildPackages (buildPackages: string[]): Promise<boolean> {
  if (!buildPackages.length) {
    globalInfo('All packages were added to allowBuilds with value false.')
    return true
  }
  return exitOnPromptCancel(() => confirm({
    message: `The next packages will now be built: ${buildPackages.join(', ')}.\nDo you approve?`,
    default: false,
  }))
}

async function exitOnPromptCancel<Answer> (prompt: () => Promise<Answer>): Promise<Answer> {
  try {
    return await prompt()
  } catch (err) {
    if (isError(err) && err.name === 'ExitPromptError') {
      // eslint-disable-next-line n/no-process-exit -- the user cancelled the prompt, so nothing else should run
      process.exit(0)
    }
    throw err
  }
}

/**
 * Without `decided`, every ignored build of a target is cleared.
 * With it, only the ignored builds whose allowBuilds key is in `decided` are.
 */
async function clearDecidedIgnoredBuilds (targets: ApprovalTarget[], decided: Set<string> | undefined): Promise<void> {
  await Promise.all(targets.map(async ({ modulesDir, modulesManifest }) => {
    if (!modulesManifest?.ignoredBuilds) return
    if (decided == null) {
      delete modulesManifest.ignoredBuilds
    } else {
      removeDecidedIgnoredBuilds(modulesManifest.ignoredBuilds, decided)
      if (!modulesManifest.ignoredBuilds.size) delete modulesManifest.ignoredBuilds
    }
    await writeModulesManifest(modulesDir, modulesManifest as Modules)
  }))
}

function removeDecidedIgnoredBuilds (ignoredBuilds: IgnoredBuilds, decided: Set<string>): void {
  for (const depPath of ignoredBuilds) {
    if (decided.has(allowBuildKeyFromIgnoredBuild(depPath))) {
      ignoredBuilds.delete(depPath)
    }
  }
}

interface BuildApprovedPackagesOptions {
  targets: ApprovalTarget[]
  buildPackages: string[]
  allowBuilds: Record<string, boolean | string>
  commands?: CommandHandlerMap
}

async function buildApprovedPackages ({ targets, buildPackages, allowBuilds, commands }: BuildApprovedPackagesOptions): Promise<void> {
  await Promise.all(targets.map(async (target) => {
    const targetBuildPackages = buildPackages.filter((name) => target.automaticallyIgnoredBuilds?.includes(name))
    if (!targetBuildPackages.length) return
    if (target.opts.enableGlobalVirtualStore) {
      await install.handler({
        ...target.opts,
        allowBuilds,
        frozenLockfile: true,
        optimisticRepeatInstall: false,
      } as any, [], commands) // eslint-disable-line @typescript-eslint/no-explicit-any -- approve-builds options are not typed as the full install option set
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
