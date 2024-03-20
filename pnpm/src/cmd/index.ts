import pick from 'ramda/src/pick'

import {
  add,
  ci,
  dedupe,
  fetch,
  install,
  link,
  prune,
  remove,
  unlink,
  update,
  importCommand,
} from '@pnpm/plugin-commands-installation'
import {
  create,
  dlx,
  exec,
  restart,
  run,
  test,
} from '@pnpm/plugin-commands-script-runners'
import {
  catFile,
  catIndex,
  findHash,
} from '@pnpm/plugin-commands-store-inspecting'
import { env } from '@pnpm/plugin-commands-env'
import { types as allTypes } from '@pnpm/config'
import { init } from '@pnpm/plugin-commands-init'
import { audit } from '@pnpm/plugin-commands-audit'
import { store } from '@pnpm/plugin-commands-store'
import { setup } from '@pnpm/plugin-commands-setup'
import { doctor } from '@pnpm/plugin-commands-doctor'
import { deploy } from '@pnpm/plugin-commands-deploy'
import { server } from '@pnpm/plugin-commands-server'
import { rebuild } from '@pnpm/plugin-commands-rebuild'
import { licenses } from '@pnpm/plugin-commands-licenses'
import { outdated } from '@pnpm/plugin-commands-outdated'
import { list, ll, why } from '@pnpm/plugin-commands-listing'
import type { CompletionFunc, PnpmOptions } from '@pnpm/types'
import { pack, publish } from '@pnpm/plugin-commands-publishing'
import { config, getCommand, setCommand } from '@pnpm/plugin-commands-config'
import { patch, patchCommit, patchRemove } from '@pnpm/plugin-commands-patching'

import * as bin from './bin'
import * as root from './root'
import { createHelp } from './help'
import * as recursive from './recursive'
import * as installTest from './installTest'
import { createCompletion } from './completion'

export const GLOBAL_OPTIONS = pick(
  [
    'color',
    'dir',
    'filter',
    'filter-prod',
    'loglevel',
    'help',
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
    'include-workspace-root',
    'fail-if-no-match',
  ],
  allTypes
)

export type CommandResponse = string | { output?: string; exitCode: number }

export type Command =
  | ((
    opts: PnpmOptions | any, // eslint-disable-line @typescript-eslint/no-explicit-any
    params: string[]
  ) => CommandResponse | Promise<CommandResponse>)
  | ((opts: PnpmOptions | any, params: string[]) => void) // eslint-disable-line @typescript-eslint/no-explicit-any
  | ((opts: PnpmOptions | any, params: string[]) => Promise<void>) // eslint-disable-line @typescript-eslint/no-explicit-any

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
}

const commands: CommandDefinition[] = [
  add,
  audit,
  bin,
  ci,
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
  fetch,
  importCommand,
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
  server,
  setup,
  store,
  catFile,
  catIndex,
  findHash,
  test,
  unlink,
  update,
  why,
]

const handlerByCommandName: Record<string, Command> = {}

const helpByCommandName: Record<string, () => string> = {}

const cliOptionsTypesByCommandName: Record<
  string,
  () => Record<string, unknown>
> = {}

const aliasToFullName = new Map<string, string>()

const completionByCommandName: Record<string, CompletionFunc> = {}

export const shorthandsByCommandName: Record<
  string,
  Record<string, string | string[]>
> = {}

export const rcOptionsTypes: Record<string, unknown> = {}

for (let i = 0; i < commands.length; i++) {
  const {
    cliOptionsTypes,
    commandNames,
    completion,
    handler,
    help,
    rcOptionsTypes,
    shorthands,
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
  if (commandNames.length > 1) {
    const fullName = commandNames[0]
    for (let i = 1; i < commandNames.length; i++) {
      aliasToFullName.set(commandNames[i], fullName)
    }
  }
}

handlerByCommandName.help = createHelp(helpByCommandName)

handlerByCommandName.completion = createCompletion({
  cliOptionsTypesByCommandName,
  completionByCommandName,
  initialCompletion,
  shorthandsByCommandName,
  universalOptionsTypes: GLOBAL_OPTIONS,
})

function initialCompletion(): {
  name: string;
}[] {
  return Object.keys(handlerByCommandName).map((name: string): {
    name: string;
  } => {
    return { name };
  })
}

export const pnpmCmds = handlerByCommandName

export function getCliOptionsTypes(commandName: string): Record<string, unknown> {
  return cliOptionsTypesByCommandName[commandName]?.() || {}
}

export function getCommandFullName(commandName: string): string | null {
  return (
    aliasToFullName.get(commandName) ??
    (handlerByCommandName[commandName] ? commandName : null)
  )
}
