import { login, logout } from '@pnpm/auth.commands'
import { approveBuilds, ignoredBuilds, rebuild } from '@pnpm/building.commands'
import { cache } from '@pnpm/cache.commands'
import type { CommandHandlerMap, CompletionFunc } from '@pnpm/cli.command'
import { createCompletionServer, generateCompletion } from '@pnpm/cli.commands'
import { config, getCommand, setCommand } from '@pnpm/config.commands'
import { types as allTypes } from '@pnpm/config.reader'
import { audit, licenses, sbom } from '@pnpm/deps.compliance.commands'
import { docs, list, ll, outdated, peers, view, why } from '@pnpm/deps.inspection.commands'
import { selfUpdate, setup } from '@pnpm/engine.pm.commands'
import { env, runtime } from '@pnpm/engine.runtime.commands'
import {
  create,
  dlx,
  exec,
  restart,
  run,
} from '@pnpm/exec.commands'
import { add, dedupe, fetch, importCommand, install, link, prune, remove, unlink, update } from '@pnpm/installing.commands'
import { patch, patchCommit, patchRemove } from '@pnpm/patching.commands'
import { deprecate, distTag, ping, search, undeprecate, unpublish } from '@pnpm/registry-access.commands'
import { deploy, pack, publish, version } from '@pnpm/releasing.commands'
import { catFile, catIndex, findHash, store } from '@pnpm/store.commands'
import { init } from '@pnpm/workspace.commands'
import { pick } from 'ramda'

import { parseCliArgs } from '../parseCliArgs.js'
import { shorthands as universalShorthands } from '../shorthands.js'
import type { PnpmOptions } from '../types.js'
import * as bin from './bin.js'
import * as clean from './clean.js'
import * as ci from './cleanInstall.js'
import { createHelp } from './help.js'
import * as installTest from './installTest.js'
import { NOT_IMPLEMENTED_COMMAND_SET, notImplementedCommandDefinitions } from './notImplemented.js'
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
  (opts: PnpmOptions | any, params: string[], commands?: CommandHandlerMap) => CommandResponse | Promise<CommandResponse> // eslint-disable-line @typescript-eslint/no-explicit-any
) | (
  (opts: PnpmOptions | any, params: string[], commands?: CommandHandlerMap) => void // eslint-disable-line @typescript-eslint/no-explicit-any
) | (
  (opts: PnpmOptions | any, params: string[], commands?: CommandHandlerMap) => Promise<void> // eslint-disable-line @typescript-eslint/no-explicit-any
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
  /**
   * If true, this command runs on all workspace projects by default when executed inside a workspace.
   */
  recursiveByDefault?: boolean
  /**
   * If true, a same-named script in package.json takes precedence over this
   * built-in command. This applies to all command names including aliases
   * (e.g. both "clean" and "purge").
   */
  overridableByScript?: boolean
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
  deprecate,
  deploy,
  distTag,
  dlx,
  docs,
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
  login,
  logout,
  ll,
  licenses,
  outdated,
  pack,
  patch,
  patchCommit,
  patchRemove,
  peers,
  ping,
  prune,
  publish,
  unpublish,
  rebuild,
  recursive,
  remove,
  restart,
  root,
  run,
  sbom,
  setup,
  search,
  store,
  catFile,
  catIndex,
  findHash,
  undeprecate,
  unlink,
  update,
  version,
  view,
  why,
  createHelp(helpByCommandName),
  ...notImplementedCommandDefinitions,
]

const handlerByCommandName: Record<string, Command> = {}
const cliOptionsTypesByCommandName: Record<string, () => Record<string, unknown>> = {}
const aliasToFullName = new Map<string, string>()
const completionByCommandName: Record<string, CompletionFunc> = {}
const shorthandsByCommandName: Record<string, Record<string, string | string[]>> = {}
const skipPackageManagerCheckForCommandArray = ['completion-server']
const recursiveByDefaultCommandArray: string[] = []
const overridableByScriptCommandArray: string[] = []

for (let i = 0; i < commands.length; i++) {
  const {
    cliOptionsTypes,
    commandNames,
    completion,
    handler,
    help,
    shorthands,
    skipPackageManagerCheck,
    recursiveByDefault,
    overridableByScript,
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
  }
  if (skipPackageManagerCheck) {
    skipPackageManagerCheckForCommandArray.push(...commandNames)
  }
  if (recursiveByDefault) {
    recursiveByDefaultCommandArray.push(...commandNames)
  }
  if (overridableByScript) {
    overridableByScriptCommandArray.push(...commandNames)
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

export const recursiveByDefaultCommands = new Set(recursiveByDefaultCommandArray)

export const overridableByScriptCommands = new Set(overridableByScriptCommandArray)

export { NOT_IMPLEMENTED_COMMAND_SET, shorthandsByCommandName }
