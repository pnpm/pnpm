import { checkbox, Separator } from '@inquirer/prompts'
import type { CommandHandler } from '@pnpm/cli.command'
import { interactivePromptPageSize, readProjectManifestOnly } from '@pnpm/cli.utils'
import { findOutdatedGitHubActions, isGitHubActionSelector } from '@pnpm/deps.github-actions'
import type { OutdatedPackage } from '@pnpm/deps.inspection.outdated'
import type { IncludedDependencies, ProjectRootDir } from '@pnpm/types'
import chalk from 'chalk'
import { unnest } from 'ramda'

import { findOutdatedDependencies, type ProjectToUpdate } from './findOutdatedDependencies.js'
import { getUpdateChoices } from './getUpdateChoices.js'
import {
  createGitHubActionsMatcher,
  getGitHubActionsDir,
  makeIncludeDependenciesFromCLI,
  shouldUpdateGitHubActions,
  update,
  type UpdateCommandOptions,
} from './update.js'
import { describeUpToDate, runUpdatePrompt } from './updatePrompt.js'

type OutdatedGitHubAction = Awaited<ReturnType<typeof findOutdatedGitHubActions>>[number]

type UpdateChoiceGroups = ReturnType<typeof getUpdateChoices>

type FlatChoice = Separator | { name: string, value: string, short: string, disabled?: boolean | string }

export async function interactiveUpdate (
  input: string[],
  opts: UpdateCommandOptions,
  rebuildHandler?: CommandHandler
): Promise<string | undefined> {
  const include = makeIncludeDependenciesFromCLI(opts.cliOptions)
  const projects = await readProjectsToUpdate(opts)
  const [outdatedPkgsOfProjects, outdatedActions] = await Promise.all([
    findOutdatedPackagesOfProjects(projects, input, { include, opts }),
    findOutdatedActions(input, { include, opts }),
  ])
  const workspacesEnabled = !!opts.workspaceDir
  const choiceGroups = getUpdateChoices([
    ...unnest(outdatedPkgsOfProjects),
    ...outdatedActions.map(toOutdatedDependency),
  ], workspacesEnabled)
  if (choiceGroups.length === 0) {
    return describeUpToDate(opts.latest)
  }
  const updatePkgNames = await promptForDependenciesToUpdate(flattenChoiceGroups(choiceGroups))
  return update(updatePkgNames, { ...opts, interactive: false, interactiveUpdate: true }, rebuildHandler) as Promise<undefined>
}

async function readProjectsToUpdate (opts: UpdateCommandOptions): Promise<ProjectToUpdate[]> {
  if (opts.selectedProjectsGraph != null) {
    return Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)
  }
  return [
    {
      rootDir: opts.dir as ProjectRootDir,
      manifest: await readProjectManifestOnly(opts.dir, opts),
    },
  ]
}

interface FindOutdatedOptions {
  include: IncludedDependencies
  opts: UpdateCommandOptions
}

async function findOutdatedPackagesOfProjects (
  projects: ProjectToUpdate[],
  input: string[],
  { include, opts }: FindOutdatedOptions
): Promise<OutdatedPackage[][]> {
  const packageInput = input.filter((selector) => !isGitHubActionSelector(selector))
  if (input.length === 0 || packageInput.length > 0) {
    return findOutdatedDependencies(projects, packageInput, { include, opts })
  }
  return projects.map(() => [])
}

async function findOutdatedActions (
  input: string[],
  { include, opts }: FindOutdatedOptions
): Promise<OutdatedGitHubAction[]> {
  if (!shouldUpdateGitHubActions(opts, include)) return []
  return findOutdatedGitHubActions({
    compatible: opts.latest !== true,
    dir: getGitHubActionsDir(opts),
    match: createGitHubActionsMatcher(input),
    minimumReleaseAge: opts.minimumReleaseAge,
    minimumReleaseAgeExclude: opts.minimumReleaseAgeExclude,
    serverUrl: opts.updateConfig?.githubActionsServer,
  })
}

function toOutdatedDependency (action: OutdatedGitHubAction): OutdatedPackage & { dependencyType: 'githubAction' } {
  return {
    alias: action.name,
    belongsTo: 'devDependencies' as const,
    current: action.current,
    dependencyType: 'githubAction' as const,
    latestManifest: { name: action.name, version: action.latest, homepage: action.homepage },
    packageName: action.name,
    wanted: action.wanted,
  }
}

function flattenChoiceGroups (choiceGroups: UpdateChoiceGroups): FlatChoice[] {
  const flatChoices: FlatChoice[] = []
  for (const group of choiceGroups) {
    flatChoices.push(new Separator(chalk.bold(`── ${group.message} ──`)))
    for (const choice of group.choices) {
      if (choice.disabled) {
        flatChoices.push(new Separator(`  ${choice.message ?? choice.name}`))
        continue
      }
      flatChoices.push({
        name: choice.message,
        value: choice.value,
        // `name` is the rendered table row (label + versions + workspace + url)
        // that lays out a single choice during selection. After submission
        // @inquirer/prompts comma-joins each choice's `short`, which without
        // this defaults to `name` and dumps the whole table back to stdout.
        short: choice.short,
      })
    }
  }
  return flatChoices
}

async function promptForDependenciesToUpdate (choices: FlatChoice[]): Promise<string[]> {
  const message = 'Choose which dependencies to update ' +
    `(Press ${chalk.cyan('<space>')} to select, ` +
    `${chalk.cyan('<a>')} to toggle all, ` +
    `${chalk.cyan('<i>')} to invert selection)\n\nEnter to start updating. Ctrl-c to cancel.`
  return runUpdatePrompt(() => checkbox({
    choices,
    pageSize: interactivePromptPageSize(),
    message,
    required: true,
    validate: (values) => {
      if (values.length === 0) {
        return 'You must choose at least one dependency.'
      }
      return true
    },
    theme: {
      icon: { checked: '●', unchecked: '○', cursor: '❯' },
      style: {
        highlight: (text: string) => text,
      },
      keybindings: ['vim'],
    },
  }))
}
