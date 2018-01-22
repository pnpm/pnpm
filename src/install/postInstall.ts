import path = require('path')
import fs = require('mz/fs')
import {PackageJson} from '@pnpm/types'
import {fromDir as readPkgFromDir} from '../fs/readPkg'
import lifecycle = require('npm-lifecycle')
import {lifecycleLogger} from '../loggers'

function noop () {}

export default async function postInstall (
  root: string,
  opts: {
    rawNpmConfig: Object,
    initialWD: string,
    userAgent: string,
    pkgId: string,
    unsafePerm: boolean,
  }
): Promise<boolean> {
  const pkg = await readPkgFromDir(root)
  const scripts = pkg && pkg.scripts || {}

  if (!scripts['install']) {
    await checkBindingGyp(root, scripts)
  }

  const modulesDir = path.join(opts.initialWD, 'node_modules')
  const scriptsOpts = {
    rawNpmConfig: opts.rawNpmConfig,
    pkgId: opts.pkgId,
    unsafePerm: opts.unsafePerm,
    modulesDir,
    root,
  }

  await npmRunScript('preinstall', pkg, scriptsOpts)
  await npmRunScript('install', pkg, scriptsOpts)
  await npmRunScript('postinstall', pkg, scriptsOpts)

  return !!scripts['preinstall'] || !!scripts['install'] || !!scripts['postinstall']
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
    unsafePerm: boolean
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
    unsafePerm: opts.unsafePerm
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
  scripts: {},
) {
  try {
    await fs.stat(path.join(root, 'binding.gyp'))
    // if fs.stat didn't throw, it means that binding.gyp exists: the default install script is:
    scripts['install'] = 'node-gyp rebuild'
  } catch {}
}
