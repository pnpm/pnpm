import { cache } from '@pnpm/cache.commands'
import { type CompletionFunc } from '@pnpm/command'
import { types as allTypes } from '@pnpm/config'
import { approveBuilds, ignoredBuilds } from '@pnpm/exec.build-commands'
import { audit } from '@pnpm/plugin-commands-audit'
import { generateCompletion, createCompletionServer } from '@pnpm/plugin-commands-completion'
import { config, getCommand, setCommand } from '@pnpm/plugin-commands-config'
import { doctor } from '@pnpm/plugin-commands-doctor'
import { env } from '@pnpm/plugin-commands-env'
import { runtime } from '@pnpm/runtime.commands'
import { deploy } from '@pnpm/plugin-commands-deploy'
import { add, ci, dedupe, fetch, install, link, prune, remove, unlink, update, importCommand } from '@pnpm/plugin-commands-installation'
import { selfUpdate } from '@pnpm/tools.plugin-commands-self-updater'
import { list, ll, why } from '@pnpm/plugin-commands-listing'
import { licenses } from '@pnpm/plugin-commands-licenses'
import { sbom } from '@pnpm/plugin-commands-sbom'
import { outdated } from '@pnpm/plugin-commands-outdated'
import { pack, publish } from '@pnpm/plugin-commands-publishing'
import { patch, patchCommit, patchRemove } from '@pnpm/plugin-commands-patching'
import { rebuild } from '@pnpm/plugin-commands-rebuild'
import {
  create,
  dlx,
  exec,
  restart,
  run,
} from '@pnpm/plugin-commands-script-runners'
import { setup } from '@pnpm/plugin-commands-setup'
import { store } from '@pnpm/plugin-commands-store'
import { catFile, catIndex, findHash } from '@pnpm/plugin-commands-store-inspecting'
import { init } from '@pnpm/plugin-commands-init'
import { pick } from 'ramda'
import { type PnpmOptions } from '../types.js'
import { shorthands as universalShorthands } from '../shorthands.js'
import { parseCliArgs } from '../parseCliArgs.js'
import * as bin from './bin.js'
import * as clean from './clean.js'
import { createHelp } from './help.js'
import * as installTest from './installTest.js'
import * as recursive from './recursive.js'
import * as root from './root.js'

export const GLOBAL_OPTIONS = pick([
  'color',
  'dir',
  'filter',
  'filter-prod',
  'loglevel',
  'parseable',
  'prefix',
  'reporter',
  'stream',
  'aggregate-output',
  'test-pattern',
  'changed-files-ignore-pattern',
  'use-stderr',
  'ignore-workspace',
  'workspace-packages',
  'workspace-root',
  'yes',
  'include-workspace-root',
  'fail-if-no-match',
], allTypes)

export type CommandResponse = string | { output?: string, exitCode: number }

export type Command = (
  (opts: PnpmOptions | any, params: string[]) => CommandResponse | Promise<CommandResponse> // eslint-disable-line @typescript-eslint/no-explicit-any
) | (
  (opts: PnpmOptions | any, params: string[]) => void // eslint-disable-line @typescript-eslint/no-explicit-any
) | (
  (opts: PnpmOptions | any, params: string[]) => Promise<void> // eslint-disable-line @typescript-eslint/no-explicit-any
)

export interface CommandDefinition {
  /** The main logic of the command. */
  handler: Command
  /** The help text for the command that describes its usage and options. */
  help: () => string
  /** The names that will trigger this command handler. */
  commandNames: string[]
  /**
   * A function that returns an object whose keys are acceptable CLI options
   * for this command and whose values are the types of values
   * for these options for validation.
   */
  cliOptionsTypes: () => Record<string, unknown>
  /**
   * A function that returns an object whose keys are acceptable options
   * in the .npmrc file for this command and whose values are the types of values
   * for these options for validation.
   */
  rcOptionsTypes: () => Record<string, unknown>
  /** Auto-completion provider for this command. */
  completion?: CompletionFunc
  /**
   * Option names that will resolve into one or more of the other options.
   *
   * Example:
   * ```ts
   * {
   *   D: '--dev',
   *   parallel: ['--no-sort', '--recursive'],
   * }
   * ```
   */
  shorthands?: Record<string, string | string[]>
  /**
   * If true, this command should not care about what package manager is specified in the "packageManager" field of "package.json".
   */
  skipPackageManagerCheck?: boolean
}

const helpByCommandName: Record<string, () => string> = {}

const commands: CommandDefinition[] = [
  add,
  approveBuilds,
  audit,
  bin,
  cache,
  ci,
  clean,
  config,
  dedupe,
  getCommand,
  setCommand,
  create,
  deploy,
  dlx,
  doctor,
  env,
  exec,
  runtime,
  fetch,
  generateCompletion,
  ignoredBuilds,
  importCommand,
  selfUpdate,
  init,
  install,
  installTest,
  link,
  list,
  ll,
  licenses,
  outdated,
  pack,
  patch,
  patchCommit,
  patchRemove,
  prune,
  publish,
  rebuild,
  recursive,
  remove,
  restart,
  root,
  run,
  sbom,
  setup,
  store,
  catFile,
  catIndex,
  findHash,
  unlink,
  update,
  why,
  createHelp(helpByCommandName),
]

const handlerByCommandName: Record<string, Command> = {}
const cliOptionsTypesByCommandName: Record<string, () => Record<string, unknown>> = {}
const aliasToFullName = new Map<string, string>()
const completionByCommandName: Record<string, CompletionFunc> = {}
const shorthandsByCommandName: Record<string, Record<string, string | string[]>> = {}
const rcOptionsTypes: Record<string, unknown> = {}
const skipPackageManagerCheckForCommandArray = ['completion-server']

for (let i = 0; i < commands.length; i++) {
  const {
    cliOptionsTypes,
    commandNames,
    completion,
    handler,
    help,
    rcOptionsTypes,
    shorthands,
    skipPackageManagerCheck,
  } = commands[i]
  if (!commandNames || commandNames.length === 0) {
    throw new Error(`The command at index ${i} doesn't have command names`)
  }
  for (const commandName of commandNames) {
    handlerByCommandName[commandName] = handler as Command
    helpByCommandName[commandName] = help
    cliOptionsTypesByCommandName[commandName] = cliOptionsTypes
    shorthandsByCommandName[commandName] = shorthands ?? {}
    if (completion != null) {
      completionByCommandName[commandName] = completion
    }
    Object.assign(rcOptionsTypes, rcOptionsTypes())
  }
  if (skipPackageManagerCheck) {
    skipPackageManagerCheckForCommandArray.push(...commandNames)
  }
  if (commandNames.length > 1) {
    const fullName = commandNames[0]
    for (let i = 1; i < commandNames.length; i++) {
      aliasToFullName.set(commandNames[i], fullName)
    }
  }
}

handlerByCommandName['completion-server'] = createCompletionServer({
  cliOptionsTypesByCommandName,
  completionByCommandName,
  initialCompletion,
  shorthandsByCommandName,
  universalOptionsTypes: GLOBAL_OPTIONS,
  universalShorthands,
  parseCliArgs,
})

function initialCompletion (): Array<{ name: string }> {
  return Object.keys(handlerByCommandName).map((name) => ({ name }))
}

export const pnpmCmds = handlerByCommandName

export const skipPackageManagerCheckForCommand = new Set(skipPackageManagerCheckForCommandArray)

export function getCliOptionsTypes (commandName: string): Record<string, unknown> {
  return cliOptionsTypesByCommandName[commandName]?.() || {}
}

export function getCommandFullName (commandName: string): string | null {
  return aliasToFullName.get(commandName) ??
    (handlerByCommandName[commandName] ? commandName : null)
}

export { shorthandsByCommandName, rcOptionsTypes }
