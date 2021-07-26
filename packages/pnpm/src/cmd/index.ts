import { CompletionFunc } from '@pnpm/command'
import { types as allTypes } from '@pnpm/config'
import { audit } from '@pnpm/plugin-commands-audit'
import { env } from '@pnpm/plugin-commands-env'
import { importCommand } from '@pnpm/plugin-commands-import'
import { add, fetch, install, link, prune, remove, unlink, update } from '@pnpm/plugin-commands-installation'
import { list, ll, why } from '@pnpm/plugin-commands-listing'
import { outdated } from '@pnpm/plugin-commands-outdated'
import { pack, publish } from '@pnpm/plugin-commands-publishing'
import { rebuild } from '@pnpm/plugin-commands-rebuild'
import {
  exec,
  restart,
  run,
  test,
} from '@pnpm/plugin-commands-script-runners'
import { server } from '@pnpm/plugin-commands-server'
import { setup } from '@pnpm/plugin-commands-setup'
import { store } from '@pnpm/plugin-commands-store'
import pick from 'ramda/src/pick'
import { PnpmOptions } from '../types'
import * as bin from './bin'
import createCompletion from './completion'
import createHelp from './help'
import * as installTest from './installTest'
import * as recursive from './recursive'
import * as root from './root'

export const GLOBAL_OPTIONS = pick([
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
  'test-pattern',
  'use-stderr',
  'workspace-packages',
  'workspace-root',
], allTypes)

export type CommandResponse = string | { output: string, exitCode: number } | undefined

export type Command = (
  opts: PnpmOptions,
  params: string[]
) => CommandResponse | Promise<CommandResponse>

const commands: Array<{
  cliOptionsTypes: () => Object
  commandNames: string[]
  completion?: CompletionFunc
  handler: Function
  help: () => string
  rcOptionsTypes: () => Record<string, unknown>
  shorthands?: Record<string, string | string[]>
}> = [
  add,
  audit,
  bin,
  env,
  exec,
  fetch,
  importCommand,
  install,
  installTest,
  link,
  list,
  ll,
  outdated,
  pack,
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
  test,
  unlink,
  update,
  why,
]

const handlerByCommandName: Record<string, Command> = {}
const helpByCommandName: Record<string, () => string> = {}
const cliOptionsTypesByCommandName: Record<string, () => Object> = {}
const aliasToFullName: Map<string, string> = new Map()
const completionByCommandName: Record<string, CompletionFunc> = {}
const shorthandsByCommandName: Record<string, Record<string, string | string[]>> = {}
const rcOptionsTypes: Record<string, unknown> = {}

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

function initialCompletion () {
  return Object.keys(handlerByCommandName).map((name) => ({ name }))
}

export default handlerByCommandName

export function getCliOptionsTypes (commandName: string) {
  return cliOptionsTypesByCommandName[commandName]?.() || {}
}

export function getCommandFullName (commandName: string) {
  return aliasToFullName.get(commandName) ??
    (handlerByCommandName[commandName] ? commandName : null)
}

export { shorthandsByCommandName, rcOptionsTypes }
