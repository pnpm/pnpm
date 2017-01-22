import assert = require('assert')
import path = require('path')
import crossSpawn = require('cross-spawn')
import resolve = require('resolve')

export type SwitcherOptions = {
  requiredBin: string,
  bin: string,
  globalRequirePath: string,
}

export default function switcher (opts: SwitcherOptions) {
  assert(opts, 'opts is required')
  assert(opts.requiredBin, 'opts.requiredBin is required')
  assert(opts.globalRequirePath, 'opts.globalRequirePath is required')
  assert(opts.bin, 'opts.bin is required')

  const local = getLocal(opts.requiredBin)
  if (!local) {
    require(opts.globalRequirePath)
    return
  }

  console.log(`Using local install of ${opts.bin}`)
  const cmd = path.join(local.slice(0, local.lastIndexOf(`${path.sep}node_modules${path.sep}`) + 1), 'node_modules', '.bin', opts.bin)
  crossSpawn.sync(cmd, process.argv.slice(2), {stdio: 'inherit'})
}

function getLocal(requiredBin: string) {
  try {
    return resolve.sync(requiredBin, {basedir: process.cwd()})
  } catch (err) {
    return null
  }
}
