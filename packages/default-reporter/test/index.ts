/// <reference path="../../../typings/index.d.ts"/>
import { Config } from '@pnpm/config'
import {
  deprecationLogger,
  hookLogger,
  packageManifestLogger,
  rootLogger,
  skippedOptionalDependencyLogger,
  statsLogger,
  summaryLogger,
} from '@pnpm/core-loggers'
import { toOutput$ } from '@pnpm/default-reporter'
import PnpmError from '@pnpm/error'
import logger, {
  createStreamParser,
} from '@pnpm/logger'
import './reportingContext'
import './reportingErrors'
import './reportingLifecycleScripts'
import './reportingProgress'
import './reportingRequestRetry'
import './reportingScope'
import { map, skip, take } from 'rxjs/operators'
import chalk = require('chalk')
import normalizeNewline = require('normalize-newline')
import path = require('path')
import R = require('ramda')
import test = require('tape')

const WARN = chalk.bgYellow.black('\u2009WARN\u2009')
const ERROR = chalk.bgRed.black('\u2009ERROR\u2009')
const DEPRECATED = chalk.red('deprecated')
const versionColor = chalk.grey
const ADD = chalk.green('+')
const SUB = chalk.red('-')
const h1 = chalk.cyanBright

const EOL = '\n'

test('prints summary (of current package only)', t => {
  const prefix = '/home/jane/project'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as Config,
    },
    streamParser: createStreamParser(),
  })

  statsLogger.debug({ added: 5, prefix: `${prefix}/packages/foo` })
  statsLogger.debug({ removed: 1, prefix: `${prefix}/packages/foo` })
  packageManifestLogger.debug({
    initial: {
      name: 'foo',
      version: '1.0.0',

      dependencies: {
        'is-13': '^1.0.0',
      },
      devDependencies: {
        'is-negative': '^1.0.0',
      },
    },
    prefix,
  })
  deprecationLogger.debug({
    deprecated: 'This package was deprecated because bla bla bla',
    depth: 0,
    pkgId: 'registry.npmjs.org/bar/2.0.0',
    pkgName: 'bar',
    pkgVersion: '2.0.0',
    prefix,
  })
  rootLogger.debug({
    added: {
      dependencyType: 'prod',
      id: 'registry.npmjs.org/foo/1.0.0',
      latest: '2.0.0',
      name: 'foo',
      realName: 'foo',
      version: '1.0.0',
    },
    prefix,
  })
  rootLogger.debug({
    added: {
      dependencyType: 'prod',
      id: 'registry.npmjs.org/bar/2.0.0',
      latest: '1.0.0', // this won't be printed in summary because latest is less than current version
      name: 'bar',
      realName: 'bar',
      version: '2.0.0',
    },
    prefix,
  })
  rootLogger.debug({
    prefix,
    removed: {
      dependencyType: 'prod',
      name: 'foo',
      version: '0.1.0',
    },
  })
  rootLogger.debug({
    added: {
      dependencyType: 'dev',
      id: 'registry.npmjs.org/qar/2.0.0',
      name: 'qar',
      realName: 'qar',
      version: '2.0.0',
    },
    prefix,
  })
  // This log is going to be ignored because it is not in the current prefix
  rootLogger.debug({
    added: {
      dependencyType: 'optional',
      id: 'registry.npmjs.org/lala/2.0.0',
      name: 'lala',
      realName: 'lala',
      version: '2.0.0',
    },
    prefix: `${prefix}/packages/foo`,
  })
  rootLogger.debug({
    added: {
      dependencyType: 'optional',
      id: 'registry.npmjs.org/lala/1.1.0',
      name: 'lala',
      realName: 'lala',
      version: '1.1.0',
    },
    prefix,
  })
  rootLogger.debug({
    prefix,
    removed: {
      dependencyType: 'optional',
      name: 'is-positive',
    },
  })
  rootLogger.debug({
    added: {
      dependencyType: 'optional',
      linkedFrom: '/src/is-linked',
      name: 'is-linked',
      realName: 'is-linked',
    },
    prefix,
  })
  rootLogger.debug({
    added: {
      dependencyType: 'prod',
      id: 'registry.npmjs.org/winst0n/2.0.0',
      latest: '1.0.0',
      name: 'winston',
      realName: 'winst0n',
      version: '1.0.0',
    },
    prefix,
  })
  packageManifestLogger.debug({
    prefix,
    updated: {
      dependencies: {
        'is-negative': '^1.0.0',
      },
      devDependencies: {
        'is-13': '^1.0.0',
      },
    },
  })
  rootLogger.debug({
    added: {
      linkedFrom: '/src/is-linked2',
      name: 'is-linked2',
      realName: 'is-linked2',
    },
    prefix,
  })
  summaryLogger.debug({ prefix })

  t.plan(1)

  output$.pipe(skip(2), take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output,
        `packages/foo                             |   ${chalk.green('+5')}   ${chalk.red('-1')} ${ADD + SUB}${EOL}` +
        `${WARN} ${DEPRECATED} bar@2.0.0: This package was deprecated because bla bla bla${EOL}${EOL}` +
        `\
${h1('dependencies:')}
${ADD} bar ${versionColor('2.0.0')} ${DEPRECATED}
${SUB} foo ${versionColor('0.1.0')}
${ADD} foo ${versionColor('1.0.0')} ${versionColor('(2.0.0 is available)')}
${SUB} is-13 ${versionColor('^1.0.0')}
${ADD} is-negative ${versionColor('^1.0.0')}
${ADD} winston <- winst0n ${versionColor('1.0.0')}

${h1('optionalDependencies:')}
${ADD} is-linked ${chalk.grey(`<- ${path.relative(prefix, '/src/is-linked')}`)}
${SUB} is-positive
${ADD} lala ${versionColor('1.1.0')}

${h1('devDependencies:')}
${ADD} is-13 ${versionColor('^1.0.0')}
${SUB} is-negative ${versionColor('^1.0.0')}
${ADD} qar ${versionColor('2.0.0')}

${h1('node_modules:')}
${ADD} is-linked2 ${chalk.grey(`<- ${path.relative(prefix, '/src/is-linked2')}`)}
`)
    },
  })
})

test('prints summary for global installation', t => {
  const prefix = '/home/jane/.nvs/node/10.0.0/x64/pnpm-global/1'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: {
        dir: prefix,
        global: true,
      } as Config,
    },
    streamParser: createStreamParser(),
  })

  rootLogger.debug({
    added: {
      dependencyType: 'prod',
      id: 'registry.npmjs.org/foo/1.0.0',
      latest: '2.0.0',
      name: 'foo',
      realName: 'foo',
      version: '1.0.0',
    },
    prefix,
  })
  rootLogger.debug({
    added: {
      dependencyType: 'prod',
      id: 'registry.npmjs.org/bar/2.0.0',
      latest: '1.0.0', // this won't be printed in summary because latest is less than current version
      name: 'bar',
      realName: 'bar',
      version: '2.0.0',
    },
    prefix,
  })
  packageManifestLogger.debug({
    prefix,
    updated: {
      dependencies: {
        'is-negative': '^1.0.0',
      },
      devDependencies: {
        'is-13': '^1.0.0',
      },
    },
  })
  summaryLogger.debug({ prefix })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, EOL + `\
${h1(`${prefix}:`)}
${ADD} bar ${versionColor('2.0.0')}
${ADD} foo ${versionColor('1.0.0')} ${versionColor('(2.0.0 is available)')}
`)
    },
  })
})

test('prints added peer dependency', t => {
  const prefix = '/home/jane/.nvs/node/10.0.0/x64/pnpm-global/1'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: {
        dir: prefix,
      } as Config,
    },
    streamParser: createStreamParser(),
  })

  packageManifestLogger.debug({
    initial: {},
    prefix,
  })
  packageManifestLogger.debug({
    prefix,
    updated: {
      devDependencies: {
        'is-negative': '^1.0.0',
      },
      peerDependencies: {
        'is-negative': '^1.0.0',
      },
    },
  })
  summaryLogger.debug({ prefix })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, EOL + `\
${h1('peerDependencies:')}
${ADD} is-negative ${versionColor('^1.0.0')}

${h1('devDependencies:')}
${ADD} is-negative ${versionColor('^1.0.0')}
`)
    },
  })
})

test('prints summary correctly when the same package is specified both in optional and prod dependencies', t => {
  const prefix = '/home/jane/.nvs/node/10.0.0/x64/pnpm-global/1'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: {
        dir: prefix,
      } as Config,
    },
    streamParser: createStreamParser(),
  })

  packageManifestLogger.debug({
    initial: {
      name: 'foo',
      version: '1.0.0',

      dependencies: {
        bar: '^2.0.0',
        foo: '^1.0.0',
      },
      optionalDependencies: {
        foo: '^1.0.0',
      },
    },
    prefix,
  })
  rootLogger.debug({
    added: {
      dependencyType: 'prod',
      id: 'registry.npmjs.org/bar/2.0.0',
      name: 'bar',
      realName: 'bar',
      version: '2.0.0',
    },
    prefix,
  })
  packageManifestLogger.debug({
    prefix,
    updated: {
      dependencies: {
        bar: '^2.0.0',
      },
      optionalDependencies: {
        foo: '^1.0.0',
      },
    },
  })
  summaryLogger.debug({ prefix })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, EOL + `\
${h1('dependencies:')}
${ADD} bar ${versionColor('2.0.0')}
`)
    },
  })
})

test('prints summary when some packages fail', async (t) => {
  const output$ = toOutput$({
    context: { argv: ['run'], config: { recursive: true } as Config },
    streamParser: createStreamParser(),
  })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, EOL + `Summary: ${chalk.red('6 fails')}, 7 passes

/a:
${ERROR} ${chalk.red('a failed')}

/b:
${ERROR} ${chalk.red('b failed')}

/c:
${ERROR} ${chalk.red('c failed')}

/d:
${ERROR} ${chalk.red('d failed')}

/e:
${ERROR} ${chalk.red('e failed')}

/f:
${ERROR} ${chalk.red('f failed')}`)
    },
  })

  const err = new PnpmError('RECURSIVE_FAIL', '...')
  err['fails'] = [
    {
      message: 'a failed',
      prefix: '/a',
    },
    {
      message: 'b failed',
      prefix: '/b',
    },
    {
      message: 'c failed',
      prefix: '/c',
    },
    {
      message: 'd failed',
      prefix: '/d',
    },
    {
      message: 'e failed',
      prefix: '/e',
    },
    {
      message: 'f failed',
      prefix: '/f',
    },
  ]
  err['passes'] = 7
  logger.error(err, err)
})

test('prints info', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  logger.info({ message: 'info message', prefix: process.cwd() })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, 'info message')
    },
  })
})

test('prints added/removed stats during installation', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })
  const prefix = process.cwd()

  statsLogger.debug({ added: 5, prefix })
  statsLogger.debug({ removed: 1, prefix })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `Packages: ${chalk.green('+5')} ${chalk.red('-1')}
${ADD + ADD + ADD + ADD + ADD + SUB}`
      )
    },
  })
})

test('prints added/removed stats during installation when 0 removed', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })
  const prefix = process.cwd()

  statsLogger.debug({ added: 2, prefix })
  statsLogger.debug({ removed: 0, prefix })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `Packages: ${chalk.green('+2')}
${ADD + ADD}`
      )
    },
  })
})

test('prints only the added stats if nothing was removed', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })
  const prefix = process.cwd()

  statsLogger.debug({ removed: 0, prefix })
  statsLogger.debug({ added: 1, prefix })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `Packages: ${chalk.green('+1')}
${ADD}`)
    },
  })
})

test('prints only the removed stats if nothing was added', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })
  const prefix = process.cwd()

  statsLogger.debug({ removed: 1, prefix })
  statsLogger.debug({ added: 0, prefix })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `Packages: ${chalk.red('-1')}
${SUB}`)
    },
  })
})

test('prints only the added stats if nothing was removed and a lot added', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    reportingOptions: { outputMaxWidth: 20 },
    streamParser: createStreamParser(),
  })
  const prefix = process.cwd()

  statsLogger.debug({ removed: 0, prefix })
  statsLogger.debug({ added: 100, prefix })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `Packages: ${chalk.green('+100')}
${R.repeat(ADD, 20).join('')}`)
    },
  })
})

test('prints only the removed stats if nothing was added and a lot removed', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    reportingOptions: { outputMaxWidth: 20 },
    streamParser: createStreamParser(),
  })
  const prefix = process.cwd()

  statsLogger.debug({ removed: 100, prefix })
  statsLogger.debug({ added: 0, prefix })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `Packages: ${chalk.red('-100')}
${R.repeat(SUB, 20).join('')}`)
    },
  })
})

test('prints at least one remove sign when removed !== 0', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    reportingOptions: { outputMaxWidth: 20 },
    streamParser: createStreamParser(),
  })
  const prefix = process.cwd()

  statsLogger.debug({ removed: 1, prefix })
  statsLogger.debug({ added: 100, prefix })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `Packages: ${chalk.green('+100')} ${chalk.red('-1')}
${R.repeat(ADD, 19).join('') + SUB}`
      )
    },
  })
})

test('prints at least one add sign when added !== 0', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    reportingOptions: { outputMaxWidth: 20 },
    streamParser: createStreamParser(),
  })
  const prefix = process.cwd()

  statsLogger.debug({ removed: 100, prefix })
  statsLogger.debug({ added: 1, prefix })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `Packages: ${chalk.green('+1')} ${chalk.red('-100')}
${ADD + R.repeat(SUB, 19).join('')}`)
    },
  })
})

test('prints just removed during uninstallation', t => {
  const output$ = toOutput$({
    context: { argv: ['remove'] },
    streamParser: createStreamParser(),
  })
  const prefix = process.cwd()

  statsLogger.debug({ removed: 4, prefix })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `Packages: ${chalk.red('-4')}
${SUB + SUB + SUB + SUB}`)
    },
  })
})

test('prints added/removed stats and warnings during recursive installation', t => {
  const rootPrefix = '/home/jane/repo'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: rootPrefix, recursive: true } as Config,
    },
    streamParser: createStreamParser(),
  })

  logger.warn({ message: 'Some issue', prefix: '/home/jane/repo/pkg-5' })
  logger.warn({ message: 'Some other issue', prefix: rootPrefix })
  statsLogger.debug({ removed: 1, prefix: '/home/jane/repo' })
  statsLogger.debug({ added: 0, prefix: '/home/jane/repo' })
  statsLogger.debug({ removed: 0, prefix: '/home/jane/repo/pkg-5' })
  statsLogger.debug({ added: 0, prefix: '/home/jane/repo/pkg-5' })
  statsLogger.debug({ added: 2, prefix: '/home/jane/repo/dir/pkg-2' })
  statsLogger.debug({ added: 5, prefix: '/home/jane/repo/pkg-1' })
  statsLogger.debug({ removed: 1, prefix: '/home/jane/repo/pkg-1' })
  deprecationLogger.debug({
    deprecated: 'This package was deprecated because bla bla bla',
    depth: 0,
    pkgId: 'registry.npmjs.org/bar/2.0.0',
    pkgName: 'bar',
    pkgVersion: '2.0.0',
    prefix: '/home/jane/repo/dir/pkg-2',
  })
  statsLogger.debug({ removed: 0, prefix: '/home/jane/repo/dir/pkg-2' })
  statsLogger.debug({ removed: 0, prefix: '/home/jane/repo/loooooooooooooooooooooooooooooooooong/pkg-3' })
  statsLogger.debug({ added: 1, prefix: '/home/jane/repo/loooooooooooooooooooooooooooooooooong/pkg-3' })
  statsLogger.debug({ removed: 1, prefix: '/home/jane/repo/loooooooooooooooooooooooooooooooooong-pkg-4' })
  statsLogger.debug({ added: 0, prefix: '/home/jane/repo/loooooooooooooooooooooooooooooooooong-pkg-4' })
  deprecationLogger.debug({
    deprecated: 'This package was deprecated because bla bla bla',
    depth: 0,
    pkgId: 'registry.npmjs.org/foo/1.0.0',
    pkgName: 'foo',
    pkgVersion: '1.0.0',
    prefix: rootPrefix,
  })

  t.plan(1)

  output$.pipe(skip(8), take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `\
pkg-5                                    | ${WARN} Some issue
.                                        | ${WARN} Some other issue
.                                        |   ${chalk.red('-1')} ${SUB}
pkg-1                                    |   ${chalk.green('+5')}   ${chalk.red('-1')} ${ADD + SUB}
dir/pkg-2                                | ${WARN} ${DEPRECATED} bar@2.0.0
dir/pkg-2                                |   ${chalk.green('+2')} ${ADD}
.../pkg-3                                |   ${chalk.green('+1')} ${ADD}
...ooooooooooooooooooooooooooooong-pkg-4 |   ${chalk.red('-1')} ${SUB}
.                                        | ${WARN} ${DEPRECATED} foo@1.0.0`)
    },
  })
})

test('recursive installation: prints only the added stats if nothing was removed and a lot added', t => {
  const output$ = toOutput$({
    context: {
      argv: ['recursive'],
      config: { dir: '/home/jane/repo' } as Config,
    },
    reportingOptions: { outputMaxWidth: 60 },
    streamParser: createStreamParser(),
  })

  statsLogger.debug({ removed: 0, prefix: '/home/jane/repo/pkg-1' })
  statsLogger.debug({ added: 190, prefix: '/home/jane/repo/pkg-1' })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `pkg-1                                    | ${chalk.green('+190')} ${R.repeat(ADD, 12).join('')}`)
    },
  })
})

test('recursive installation: prints only the removed stats if nothing was added and a lot removed', t => {
  const output$ = toOutput$({
    context: {
      argv: ['recursive'],
      config: { dir: '/home/jane/repo' } as Config,
    },
    reportingOptions: { outputMaxWidth: 60 },
    streamParser: createStreamParser(),
  })

  statsLogger.debug({ removed: 190, prefix: '/home/jane/repo/pkg-1' })
  statsLogger.debug({ added: 0, prefix: '/home/jane/repo/pkg-1' })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `pkg-1                                    | ${chalk.red('-190')} ${R.repeat(SUB, 12).join('')}`)
    },
  })
})

test('recursive installation: prints at least one remove sign when removed !== 0', t => {
  const output$ = toOutput$({
    context: {
      argv: ['recursive'],
      config: { dir: '/home/jane/repo' } as Config,
    },
    reportingOptions: { outputMaxWidth: 62 },
    streamParser: createStreamParser(),
  })

  statsLogger.debug({ removed: 1, prefix: '/home/jane/repo/pkg-1' })
  statsLogger.debug({ added: 100, prefix: '/home/jane/repo/pkg-1' })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `pkg-1                                    | ${chalk.green('+100')}   ${chalk.red('-1')} ${R.repeat(ADD, 8).join('') + SUB}`)
    },
  })
})

test('recursive installation: prints at least one add sign when added !== 0', t => {
  const output$ = toOutput$({
    context: {
      argv: ['recursive'],
      config: { dir: '/home/jane/repo' } as Config,
    },
    reportingOptions: { outputMaxWidth: 62 },
    streamParser: createStreamParser(),
  })

  statsLogger.debug({ removed: 100, prefix: '/home/jane/repo/pkg-1' })
  statsLogger.debug({ added: 1, prefix: '/home/jane/repo/pkg-1' })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `pkg-1                                    |   ${chalk.green('+1')} ${chalk.red('-100')} ${ADD + R.repeat(SUB, 8).join('')}`)
    },
  })
})

test('recursive uninstall: prints removed packages number', t => {
  const output$ = toOutput$({
    context: {
      argv: ['remove'],
      config: { dir: '/home/jane/repo', recursive: true } as Config,
    },
    reportingOptions: { outputMaxWidth: 62 },
    streamParser: createStreamParser(),
  })

  statsLogger.debug({ removed: 1, prefix: '/home/jane/repo/pkg-1' })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `pkg-1                                    |   ${chalk.red('-1')} ${SUB}`)
    },
  })
})

test('install: print hook message', t => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: '/home/jane/repo' } as Config,
    },
    streamParser: createStreamParser(),
  })

  hookLogger.debug({
    from: '/home/jane/repo/pnpmfile.js',
    hook: 'readPackage',
    message: 'foo',
    prefix: '/home/jane/repo',
  })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `${chalk.magentaBright('readPackage')}: foo`)
    },
  })
})

test('recursive: print hook message', t => {
  const output$ = toOutput$({
    context: {
      argv: ['recursive'],
      config: { dir: '/home/jane/repo' } as Config,
    },
    streamParser: createStreamParser(),
  })

  hookLogger.debug({
    from: '/home/jane/repo/pnpmfile.js',
    hook: 'readPackage',
    message: 'foo',
    prefix: '/home/jane/repo/pkg-1',
  })

  t.plan(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `pkg-1                                    | ${chalk.magentaBright('readPackage')}: foo`)
    },
  })
})

test('prints skipped optional dependency info message', t => {
  const prefix = process.cwd()
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as Config,
    },
    streamParser: createStreamParser(),
  })

  const pkgId = 'registry.npmjs.org/foo/1.0.0'

  skippedOptionalDependencyLogger.debug({
    package: {
      id: pkgId,
      name: 'foo',
      version: '1.0.0',
    },
    parents: [],
    prefix,
    reason: 'unsupported_platform',
  })

  t.plan(1)

  output$.pipe(take(1)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `info: ${pkgId} is an optional dependency and failed compatibility check. Excluding it from installation.`)
    },
  })
})

test('logLevel=default', t => {
  const prefix = process.cwd()
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as Config,
    },
    streamParser: createStreamParser(),
  })

  logger.info({ message: 'Info message', prefix })
  logger.warn({ message: 'Some issue', prefix })
  const err = new PnpmError('SOME_CODE', 'some error')
  logger.error(err, err)

  t.plan(1)

  output$.pipe(skip(2), take(1)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `Info message
${WARN} Some issue
${ERROR} ${chalk.red('some error')}`)
    },
  })
})

test('logLevel=warn', t => {
  const prefix = process.cwd()
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as Config,
    },
    reportingOptions: {
      logLevel: 'warn',
    },
    streamParser: createStreamParser(),
  })

  logger.info({ message: 'Info message', prefix })
  logger.warn({ message: 'Some issue', prefix })
  const err = new PnpmError('SOME_CODE', 'some error')
  logger.error(err, err)

  t.plan(1)

  output$.pipe(skip(1), take(1)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `${WARN} Some issue
${ERROR} ${chalk.red('some error')}`)
    },
  })
})

test('logLevel=error', t => {
  const prefix = process.cwd()
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as Config,
    },
    reportingOptions: {
      logLevel: 'error',
    },
    streamParser: createStreamParser(),
  })

  logger.info({ message: 'Info message', prefix })
  logger.warn({ message: 'Some issue', prefix })
  const err = new PnpmError('SOME_CODE', 'some error')
  logger.error(err, err)

  t.plan(1)

  output$.pipe(take(1)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `${ERROR} ${chalk.red('some error')}`)
    },
  })
})

test('warnings are collapsed', t => {
  const prefix = process.cwd()
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as Config,
    },
    reportingOptions: {
      logLevel: 'warn',
    },
    streamParser: createStreamParser(),
  })

  logger.warn({ message: 'Some issue 1', prefix })
  logger.warn({ message: 'Some issue 2', prefix })
  logger.warn({ message: 'Some issue 3', prefix })
  logger.warn({ message: 'Some issue 4', prefix })
  logger.warn({ message: 'Some issue 5', prefix })
  logger.warn({ message: 'Some issue 6', prefix })
  logger.warn({ message: 'Some issue 7', prefix })

  t.plan(1)

  output$.pipe(skip(6), take(1)).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `${WARN} Some issue 1
${WARN} Some issue 2
${WARN} Some issue 3
${WARN} Some issue 4
${WARN} Some issue 5
${WARN} 2 other warnings`)
    },
  })
})
