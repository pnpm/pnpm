import path = require('path')
import findUp = require('find-up')
import fs = require('mz/fs')
import runScript from '../runScript'
import {fromDir as readPkgFromDir} from '../fs/readPkg'

const pnpmNodeModules = findUp.sync('node_modules', {cwd: __dirname})
const nodeGyp = path.resolve(pnpmNodeModules, 'node-gyp/bin/node-gyp.js')

export default async function postInstall (
  root: string,
  log: Function,
  opts: {
    userAgent: string
  }
) {
  const pkg = await readPkgFromDir(root)
  const scripts = pkg && pkg.scripts || {}

  if (!scripts['install']) {
    await checkBindingGyp(root, log, opts)
  }

  if (scripts['install']) {
    await npmRunScript('install')
    return
  }
  await npmRunScript('preinstall')
  await npmRunScript('postinstall')
  return

  async function npmRunScript (scriptName: string) {
    if (!scripts[scriptName]) return
    return runScript('npm', ['run', scriptName], { cwd: root, log, userAgent: opts.userAgent })
  }
}

/**
 * Run node-gyp when binding.gyp is available. Only do this when there's no
 * `install` script (see `npm help scripts`).
 */
async function checkBindingGyp (
  root: string,
  log: Function,
  opts: {
    userAgent: string
  }
) {
  try {
    await fs.stat(path.join(root, 'binding.gyp'))
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code === 'ENOENT') {
      return
    }
  }
  return runScript(nodeGyp, ['rebuild'], { cwd: root, log, userAgent: opts.userAgent })
}
