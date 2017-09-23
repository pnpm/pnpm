import path = require('path')
import findUp = require('find-up')
import fs = require('mz/fs')
import runScript from '../runScript'
import {fromDir as readPkgFromDir} from '../fs/readPkg'
import lifecycle = require('@zkochan/npm-lifecycle')
import logger, {
  lifecycleLogger,
} from 'pnpm-logger'

const pnpmNodeModules = findUp.sync('node_modules', {cwd: __dirname})
const nodeGyp = path.resolve(pnpmNodeModules, 'node-gyp/bin/node-gyp.js')

function noop () {}

export default async function postInstall (
  root: string,
  opts: {
    rawNpmConfig: Object,
    initialWD: string,
    userAgent: string,
    pkgId: string,
  }
) {
  const dir = path.join(opts.initialWD, 'node_modules')
  const pkg = await readPkgFromDir(root)
  const scripts = pkg && pkg.scripts || {}

  if (!scripts['install']) {
    await checkBindingGyp(root, opts)
  }

  if (scripts['install']) {
    await npmRunScript('install')
    return
  }
  await npmRunScript('preinstall')
  await npmRunScript('postinstall')
  return

  async function npmRunScript (stage: string) {
    if (!scripts[stage]) return
    return lifecycle(pkg, stage, root, {
      dir,
      config: opts.rawNpmConfig,
      stdio: 'pipe',
      log: {
        silent: true,
        info: noop,
        warn: noop,
        silly: npmLog,
        verbose: npmLog,
        pause: noop,
        resume: noop,
        clearProgress: noop,
        showProgress: noop,
      },
    })

    function npmLog (prefix: string, logid: string, stdtype: string, line: string) {
      switch (stdtype) {
        case 'stdout':
          lifecycleLogger.info({
            script: stage,
            line: line.toString(),
            pkgId: opts.pkgId,
          })
          return
        case 'stderr':
          lifecycleLogger.error({
            script: stage,
            line: line.toString(),
            pkgId: opts.pkgId,
          })
          return
        case 'Returned: code:':
          const code = arguments[3]
          lifecycleLogger[code === 0 ? 'info' : 'error']({
            pkgId: opts.pkgId,
            script: stage,
            exitCode: code,
          })
          return
      }
    }
  }
}

/**
 * Run node-gyp when binding.gyp is available. Only do this when there's no
 * `install` script (see `npm help scripts`).
 */
async function checkBindingGyp (
  root: string,
  opts: {
    userAgent: string,
    pkgId: string,
  }
) {
  try {
    await fs.stat(path.join(root, 'binding.gyp'))
  } catch (err) {
    if ((<NodeJS.ErrnoException>err).code === 'ENOENT') {
      return
    }
  }
  return runScript(nodeGyp, ['rebuild'], {
    cwd: root,
    pkgId: opts.pkgId,
    userAgent: opts.userAgent,
  })
}
