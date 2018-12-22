import { lifecycleLogger } from '@pnpm/core-loggers'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import { PackageJson } from '@pnpm/types'
import lifecycle = require('@zkochan/npm-lifecycle')
import path = require('path')
import exists = require('path-exists')

function noop () {} // tslint:disable-line:no-empty

export async function runPostinstallHooks (
  opts: {
    depPath: string,
    optional?: boolean,
    pkgRoot: string,
    prepare?: boolean,
    rawNpmConfig: object,
    rootNodeModulesDir: string,
    unsafePerm: boolean,
  },
): Promise<boolean> {
  const pkg = await readPackageJsonFromDir(opts.pkgRoot)
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
    optional?: boolean,
    pkgRoot: string,
    rawNpmConfig: object,
    rootNodeModulesDir: string,
    stdio?: string,
    unsafePerm: boolean,
  },
) {
  const optional = opts.optional === true
  if (opts.stdio !== 'inherit') {
    lifecycleLogger.debug({
      depPath: opts.depPath,
      optional,
      script: pkg.scripts![stage],
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
          optional,
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
