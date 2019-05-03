import help from './help'
import importCmd from './import'
import install from './install'
import installTest from './installTest'
import link from './link'
import list from './list'
import outdated from './outdated'
import prune from './prune'
import publish from './publish'
import rebuild from './rebuild'
import recursive from './recursive'
import root from './root'
import run, { restart, start, stop, test } from './run'
import server from './server'
import store from './store'
import uninstall from './uninstall'
import unlink from './unlink'
import update from './update'

export default {
  help,
  import: importCmd,
  install,
  installTest,
  link,
  list,
  outdated,
  prune,
  publish,
  rebuild,
  recursive,
  restart,
  root,
  run,
  server,
  start,
  stop,
  store,
  test,
  uninstall,
  unlink,
  update,
}
