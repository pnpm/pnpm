import { lifecycleLogger } from '@pnpm/core-loggers'
import { DependencyManifest, ImporterManifest } from '@pnpm/types'
import lifecycle = require('@zkochan/npm-lifecycle')

function noop () {} // tslint:disable-line:no-empty

export default async function runLifecycleHook (
  stage: string,
  manifest: ImporterManifest | DependencyManifest,
  opts: {
    args?: string[],
    depPath: string,
    extraBinPaths?: string[],
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
      script: manifest.scripts![stage],
      stage,
      wd: opts.pkgRoot,
    })
  }

  const m = { _id: getId(manifest), ...manifest }
  m.scripts = { ...m.scripts }

  if (stage === 'start' && !m.scripts.start) {
    m.scripts.start = 'node server.js'
  }
  if (opts.args && opts.args.length && m.scripts && m.scripts[stage]) {
    m.scripts[stage] = `${m.scripts[stage]} ${opts.args.map((arg) => `"${arg}"`).join(' ')}`
  }
  return lifecycle(m, stage, opts.pkgRoot, {
    config: opts.rawNpmConfig,
    dir: opts.rootNodeModulesDir,
    extraBinPaths: opts.extraBinPaths || [],
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

function getId (manifest: ImporterManifest | DependencyManifest) {
  if (!manifest.name) {
    return undefined
  }
  if (!manifest.version) {
    return manifest.name
  }
  return `${manifest.name}@${manifest.version}`
}
