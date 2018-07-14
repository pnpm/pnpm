import {PackageJson} from '@pnpm/types'
import lifecycle = require('npm-lifecycle')
import path = require('path')
import exists = require('path-exists')
import readPackageJsonCB = require('read-package-json')
import promisify = require('util.promisify')
import {LifecycleLog, lifecycleLogger} from './logger'

const readPackageJson = promisify(readPackageJsonCB)

function noop () {} // tslint:disable-line:no-empty

export {LifecycleLog}

export async function runPostinstallHooks (
  opts: {
    depPath: string,
    rootNodeModulesDir: string,
    rawNpmConfig: object,
    pkgRoot: string,
    prepare?: boolean,
    unsafePerm: boolean,
  },
): Promise<boolean> {
  const pkg = await readPackageJson(path.join(opts.pkgRoot, 'package.json'))
  const scripts = pkg && pkg.scripts || {}

  if (!scripts.install) {
    await checkBindingGyp(opts.pkgRoot, scripts)
  }

  if (scripts.preinstall) {
    await runLifecycleHook('preinstall', pkg, opts)
  }
  if (scripts.install) {
    await runLifecycleHook('install', pkg, opts)
  }
  if (scripts.postinstall) {
    await runLifecycleHook('postinstall', pkg, opts)
  }

  if (opts.prepare && scripts.prepare) {
    await runLifecycleHook('prepare', pkg, opts)
  }

  return !!scripts.preinstall || !!scripts.install || !!scripts.postinstall
}

export default async function runLifecycleHook (
  stage: string,
  pkg: PackageJson,
  opts: {
    depPath: string,
    rootNodeModulesDir: string,
    rawNpmConfig: object,
    pkgRoot: string,
    stdio?: string,
    unsafePerm: boolean,
  },
) {
  if (opts.stdio !== 'inherit') {
    lifecycleLogger.debug({
      depPath: opts.depPath,
      script: pkg!.scripts![stage],
      stage,
      wd: opts.pkgRoot,
    })
  }

  return lifecycle(pkg, stage, opts.pkgRoot, {
    config: opts.rawNpmConfig,
    dir: opts.rootNodeModulesDir,
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
      case 'stderr':
        lifecycleLogger.debug({
          depPath: opts.depPath,
          line: line.toString(),
          stage,
          stdio: stdtype,
          wd: opts.pkgRoot,
        })
        return
      case 'Returned: code:':
        if (opts.stdio === 'inherit') {
          // Preventing the pnpm reporter from overriding the project's script output
          return
        }
        const code = arguments[3]
        lifecycleLogger.debug({
          depPath: opts.depPath,
          exitCode: code,
          stage,
          wd: opts.pkgRoot,
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
  if (await exists(path.join(root, 'binding.gyp'))) {
    scripts['install'] = 'node-gyp rebuild' // tslint:disable-line:no-string-literal
  }
}
