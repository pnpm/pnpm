import * as add from './add'
import * as audit from './audit'
import createHelp from './help'
import * as importCmd from './import'
import * as install from './install'
import * as installTest from './installTest'
import * as link from './link'
import * as list from './list'
import * as outdated from './outdated'
import * as pack from './pack'
import * as prune from './prune'
import * as publish from './publish'
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
import * as why from './why'

const commands: Array<{ commandNames: string[], handler: Function, help: () => string}> = [
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

const handlerByCommandName: Record<string, Function> = {}
const helpByCommandName: Record<string, () => string> = {}

for (const { commandNames, handler, help } of commands) {
  if (commandNames.length === 0) throw new Error('One of the commands doesn\'t have command names')
  for (const commandName of commandNames) {
    handlerByCommandName[commandName] = handler
    helpByCommandName[commandName] = help
  }
}

handlerByCommandName.help = createHelp(helpByCommandName)

export default handlerByCommandName
