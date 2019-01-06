import { lifecycleLogger } from '@pnpm/core-loggers'
import { PackageJson } from '@pnpm/types'
import lifecycle = require('@zkochan/npm-lifecycle')

function noop () {} // tslint:disable-line:no-empty

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
    runConcurrently: true,
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
