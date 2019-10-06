import help from './help'
import importCmd from './import'
import install from './install'
import installTest from './installTest'
import link from './link'
import list from './list'
import outdated from './outdated'
import prune from './prune'
import publish, { pack } from './publish'
import rebuild from './rebuild'
import recursive from './recursive'
import remove from './remove'
import root from './root'
import run, { restart, start, stop, test } from './run'
import server from './server'
import store from './store'
import unlink from './unlink'
import update from './update'

export default {
  add: install,
  help,
  import: importCmd,
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
  why: list,
}
