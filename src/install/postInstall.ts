import path = require('path')
import findUp = require('find-up')
import fs = require('mz/fs')
import {PackageJson} from '@pnpm/types'
import runScript from '../runScript'
import {fromDir as readPkgFromDir} from '../fs/readPkg'
import lifecycle = require('@zkochan/npm-lifecycle')
import logger from '@pnpm/logger'
import {lifecycleLogger} from '../loggers'

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
  const pkg = await readPkgFromDir(root)
  const scripts = pkg && pkg.scripts || {}

  if (!scripts['install']) {
    await checkBindingGyp(root, opts)
  }

  const modulesDir = path.join(opts.initialWD, 'node_modules')
  const scriptsOpts = {
    rawNpmConfig: opts.rawNpmConfig,
    pkgId: opts.pkgId,
    modulesDir,
    root,
  }

  await npmRunScript('preinstall', pkg, scriptsOpts)
  await npmRunScript('install', pkg, scriptsOpts)
  await npmRunScript('postinstall', pkg, scriptsOpts)
  return
}

export async function npmRunScript (
  stage: string,
  pkg: PackageJson,
  opts: {
    rawNpmConfig: Object,
    pkgId: string,
    modulesDir: string,
    root: string,
    stdio?: string,
  }
) {
  if (!pkg.scripts || !pkg.scripts[stage]) return
  return lifecycle(pkg, stage, opts.root, {
    dir: opts.modulesDir,
    config: opts.rawNpmConfig,
    stdio: opts.stdio || 'pipe',
    log: {
      level: opts.stdio === 'inherit' ? undefined : 'silent',
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
        if (opts.stdio === 'inherit') {
          // Preventing the pnpm reporter from overriding the project's script output
          return
        }
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
