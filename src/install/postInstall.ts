import {PackageJson} from '@pnpm/types'
import fs = require('mz/fs')
import lifecycle = require('npm-lifecycle')
import path = require('path')
import {fromDir as readPkgFromDir} from '../fs/readPkg'
import {lifecycleLogger} from '../loggers'

function noop () {} // tslint:disable-line:no-empty

export default async function postInstall (
  root: string,
  opts: {
    initialWD: string,
    pkgId: string,
    rawNpmConfig: object,
    unsafePerm: boolean,
    userAgent: string,
  },
): Promise<boolean> {
  const pkg = await readPkgFromDir(root)
  const scripts = pkg && pkg.scripts || {}

  if (!scripts['install']) { // tslint:disable-line:no-string-literal
    await checkBindingGyp(root, scripts)
  }

  const modulesDir = path.join(opts.initialWD, 'node_modules')
  const scriptsOpts = {
    modulesDir,
    pkgId: opts.pkgId,
    rawNpmConfig: opts.rawNpmConfig,
    root,
    unsafePerm: opts.unsafePerm,
  }

  await npmRunScript('preinstall', pkg, scriptsOpts)
  await npmRunScript('install', pkg, scriptsOpts)
  await npmRunScript('postinstall', pkg, scriptsOpts)

  return !!scripts['preinstall'] || !!scripts['install'] || !!scripts['postinstall'] // tslint:disable-line:no-string-literal
}

export async function npmRunScript (
  stage: string,
  pkg: PackageJson,
  opts: {
    modulesDir: string,
    pkgId: string,
    rawNpmConfig: object,
    root: string,
    stdio?: string,
    unsafePerm: boolean,
  },
) {
  if (!pkg.scripts || !pkg.scripts[stage]) return
  return lifecycle(pkg, stage, opts.root, {
    config: opts.rawNpmConfig,
    dir: opts.modulesDir,
    log: {
      clearProgress: noop,
      info: noop,
      level: opts.stdio === 'inherit' ? undefined : 'silent',
      pause: noop,
      resume: noop,
      showProgress: noop,
      silly: npmLog,
      verbose: npmLog,
      warn: noop,
    },
    stdio: opts.stdio || 'pipe',
    unsafePerm: opts.unsafePerm,
  })

  function npmLog (prefix: string, logid: string, stdtype: string, line: string) {
    switch (stdtype) {
      case 'stdout':
        lifecycleLogger.info({
          line: line.toString(),
          pkgId: opts.pkgId,
          script: stage,
        })
        return
      case 'stderr':
        lifecycleLogger.error({
          line: line.toString(),
          pkgId: opts.pkgId,
          script: stage,
        })
        return
      case 'Returned: code:':
        if (opts.stdio === 'inherit') {
          // Preventing the pnpm reporter from overriding the project's script output
          return
        }
        const code = arguments[3]
        lifecycleLogger[code === 0 ? 'info' : 'error']({
          exitCode: code,
          pkgId: opts.pkgId,
          script: stage,
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
  scripts: {},
) {
  try {
    await fs.stat(path.join(root, 'binding.gyp'))
    // if fs.stat didn't throw, it means that binding.gyp exists: the default install script is:
    scripts['install'] = 'node-gyp rebuild' // tslint:disable-line:no-string-literal
  } catch {} // tslint:disable-line:no-empty
}
