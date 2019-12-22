import { audit } from '@pnpm/plugin-commands-audit'
import { importCommand } from '@pnpm/plugin-commands-import'
import { add, install, link, prune, remove, unlink, update } from '@pnpm/plugin-commands-installation'
import { list, why } from '@pnpm/plugin-commands-listing'
import { outdated } from '@pnpm/plugin-commands-outdated'
import { pack, publish } from '@pnpm/plugin-commands-publishing'
import { rebuild } from '@pnpm/plugin-commands-rebuild'
import { recursive } from '@pnpm/plugin-commands-recursive'
import {
  restart,
  run,
  start,
  stop,
  test,
} from '@pnpm/plugin-commands-script-runners'
import { server } from '@pnpm/plugin-commands-server'
import { store } from '@pnpm/plugin-commands-store'
import { PnpmOptions } from '../types'
import createHelp from './help'
import * as installTest from './installTest'
import * as root from './root'

export type Command = (
  args: string[],
  opts: PnpmOptions,
  invocation?: string
) => string | void | Promise<string | void>

const commands: Array<{
  commandNames: string[],
  handler: Function,
  help: () => string,
  cliOptionsTypes: () => Object,
  rcOptionsTypes: () => Record<string, unknown>,
}> = [
  add,
  audit,
  importCommand,
  install,
  installTest,
  link,
  list,
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
  start,
  stop,
  store,
  test,
  unlink,
  update,
  why,
]

const handlerByCommandName: Record<string, Command> = {}
const helpByCommandName: Record<string, () => string> = {}
const cliOptionsTypesByCommandName: Record<string, () => Object> = {}
const rcOptionsTypesByCommandName: Record<string, () => Record<string, unknown>> = {}
const aliasToFullName: Map<string, string> = new Map()

for (let i = 0; i < commands.length; i++) {
  const {
    cliOptionsTypes,
    commandNames,
    handler,
    help,
    rcOptionsTypes,
  } = commands[i]
  if (!commandNames || commandNames.length === 0) {
    throw new Error('The command at index ' + i + " doesn't have command names")
  }
  for (const commandName of commandNames) {
    handlerByCommandName[commandName] = handler as Command
    helpByCommandName[commandName] = help
    cliOptionsTypesByCommandName[commandName] = cliOptionsTypes
    rcOptionsTypesByCommandName[commandName] = rcOptionsTypes
  }
  if (commandNames.length > 1) {
    const fullName = commandNames[0]
    for (let i = 1; i < commandNames.length; i++) {
      aliasToFullName.set(commandNames[i], fullName)
    }
  }
}

handlerByCommandName.help = createHelp(helpByCommandName)

export default handlerByCommandName

export function getCliOptionsTypes (commandName: string) {
  return cliOptionsTypesByCommandName[commandName]?.() || {}
}

export function getRCOptionsTypes (commandName: string) {
  return rcOptionsTypesByCommandName[commandName]?.() || {}
}

export function getCommandFullName (commandName: string) {
  return aliasToFullName.get(commandName) || commandName
}
