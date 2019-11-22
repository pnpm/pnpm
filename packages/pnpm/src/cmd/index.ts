import { audit } from '@pnpm/plugin-commands-audit'
import { list, why } from '@pnpm/plugin-commands-listing'
import { outdated } from '@pnpm/plugin-commands-outdated'
import { pack, publish } from '@pnpm/plugin-commands-publishing'
import { PnpmOptions } from '../types'
import * as add from './add'
import createHelp from './help'
import * as importCmd from './import'
import * as install from './install'
import * as installTest from './installTest'
import * as link from './link'
import * as prune from './prune'
import * as rebuild from './rebuild'
import * as recursive from './recursive'
import * as remove from './remove'
import * as restart from './restart'
import * as root from './root'
import * as run from './run'
import * as server from './server'
import * as start from './start'
import * as stop from './stop'
import * as store from './store'
import * as test from './test'
import * as unlink from './unlink'
import * as update from './update'

export type Command = (
  args: string[],
  opts: PnpmOptions,
  invocation?: string
) => string | void | Promise<string | void>

const commands: Array<{
  commandNames: string[],
  handler: Function,
  help: () => string,
  types: () => Object,
}> = [
  add,
  audit,
  importCmd,
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
const typesByCommandName: Record<string, () => Object> = {}
const aliasToFullName: Map<string, string> = new Map()

for (let i = 0; i < commands.length; i++) {
  const { commandNames, handler, help, types } = commands[i]
  if (!commandNames || commandNames.length === 0) {
    throw new Error('The command at index ' + i + " doesn't have command names")
  }
  for (const commandName of commandNames) {
    handlerByCommandName[commandName] = handler as Command
    helpByCommandName[commandName] = help
    typesByCommandName[commandName] = types
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

export function getTypes (commandName: string) {
  return typesByCommandName[commandName]?.() || {}
}

export function getCommandFullName (commandName: string) {
  return aliasToFullName.get(commandName) || commandName
}
