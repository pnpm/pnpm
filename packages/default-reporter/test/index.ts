///<reference path="../typings/index.d.ts"/>
import { PnpmConfigs } from '@pnpm/config'
import {
  deprecationLogger,
  hookLogger,
  lifecycleLogger,
  packageJsonLogger,
  progressLogger,
  rootLogger,
  skippedOptionalDependencyLogger,
  stageLogger,
  statsLogger,
  summaryLogger,
} from '@pnpm/core-loggers'
import logger, {
  createStreamParser,
} from '@pnpm/logger'
import chalk from 'chalk'
import { stripIndent, stripIndents } from 'common-tags'
import delay from 'delay'
import loadJsonFile from 'load-json-file'
import most = require('most')
import normalizeNewline = require('normalize-newline')
import path = require('path')
import { toOutput$ } from 'pnpm-default-reporter'
import R = require('ramda')
import StackTracey = require('stacktracey')
import test = require('tape')
import './reportingScope'

const WARN = chalk.bgYellow.black('\u2009WARN\u2009')
const ERROR = chalk.bgRed.black('\u2009ERROR\u2009')
const DEPRECATED = chalk.red('deprecated')
const versionColor = chalk.grey
const ADD = chalk.green('+')
const SUB = chalk.red('-')
const h1 = chalk.cyanBright
const hlValue = chalk.cyanBright
const hlPkgId = chalk['whiteBright']
const POSTINSTALL = hlValue('postinstall')
const PREINSTALL = hlValue(' preinstall')
const INSTALL = hlValue('    install')

const EOL = '\n'

test('prints progress beginning', t => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
    },
    streamParser: createStreamParser(),
  })

  const pkgId = 'registry.npmjs.org/foo/1.0.0'

  progressLogger.debug({
    pkgId,
    status: 'resolving_content',
  })

  t.plan(1)

  output$.take(1).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}`)
    },
  })
})

test('prints progress beginning when appendOnly is true', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    reportingOptions: {
      appendOnly: true,
    },
    streamParser: createStreamParser(),
  })

  const pkgId = 'registry.npmjs.org/foo/1.0.0'

  progressLogger.debug({
    pkgId,
    status: 'resolving_content',
  })

  t.plan(1)

  output$.take(1).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}`)
    },
  })
})

test('prints progress beginning during recursive install', t => {
  const output$ = toOutput$({
    context: { argv: ['recursive'] },
    streamParser: createStreamParser(),
  })

  const pkgId = 'registry.npmjs.org/foo/1.0.0'

  progressLogger.debug({
    pkgId,
    status: 'resolving_content',
  })

  t.plan(1)

  output$.take(1).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}`)
    },
  })
})

test('prints progress on first download', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    reportingOptions: { throttleProgress: 0 },
    streamParser: createStreamParser(),
  })

  const pkgId = 'registry.npmjs.org/foo/1.0.0'

  progressLogger.debug({
    pkgId,
    status: 'resolving_content',
  })
  progressLogger.debug({
    pkgId,
    status: 'fetched',
  })

  t.plan(1)

  output$.skip(1).take(1).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('1')}`)
    },
  })
})

test('moves fixed line to the end', async t => {
  const prefix = process.cwd()
  const output$ = toOutput$({
    context: { argv: ['install'] },
    reportingOptions: { throttleProgress: 0 },
    streamParser: createStreamParser(),
  })

  output$.skip(3).take(1).map(normalizeNewline).subscribe({
    complete: v => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `${WARN} foo` + EOL +
        `Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('1')}, done`)
    },
  })

  const pkgId = 'registry.npmjs.org/foo/1.0.0'

  progressLogger.debug({
    pkgId,
    status: 'resolving_content',
  })
  progressLogger.debug({
    pkgId,
    status: 'fetched',
  })
  logger.warn({ message: 'foo', prefix })

  await delay(0) // w/o delay warning goes below for some reason. Started to happen after switch to most

  stageLogger.debug('resolution_done')
  stageLogger.debug('importing_done')

  t.plan(1)
})

test('prints "Already up-to-date"', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const prefix = process.cwd()

  statsLogger.debug({ added: 0, prefix })
  statsLogger.debug({ removed: 0, prefix })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        Already up-to-date
      `)
    },
  })
})

test('prints summary (of current package only)', t => {
  const prefix = '/home/jane/project'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      configs: { prefix } as PnpmConfigs,
    },
    streamParser: createStreamParser(),
  })

  statsLogger.debug({ added: 5, prefix: `${prefix}/packages/foo` })
  statsLogger.debug({ removed: 1, prefix: `${prefix}/packages/foo` })
  packageJsonLogger.debug({
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
  packageJsonLogger.debug({
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

  output$.skip(2).take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output,
        `packages/foo                             |   ${chalk.green('+5')}   ${chalk.red('-1')} ${ADD + SUB}${EOL}` +
        `${WARN} ${DEPRECATED} bar@2.0.0: This package was deprecated because bla bla bla${EOL}${EOL}` +
        stripIndents`
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
        ` + '\n')
    },
  })
})

test('prints summary for global installation', t => {
  const prefix = '/home/jane/.nvs/node/10.0.0/x64/pnpm-global/1'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      configs: {
        global: true,
        prefix,
      } as PnpmConfigs,
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
  packageJsonLogger.debug({
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, EOL + stripIndents`
        ${h1(`${prefix}:`)}
        ${ADD} bar ${versionColor('2.0.0')}
        ${ADD} foo ${versionColor('1.0.0')} ${versionColor('(2.0.0 is available)')}
        ` + '\n')
    },
  })
})

test('prints summary correctly when the same package is specified both in optional and prod dependencies', t => {
  const prefix = '/home/jane/.nvs/node/10.0.0/x64/pnpm-global/1'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      configs: {
        prefix,
      } as PnpmConfigs,
    },
    streamParser: createStreamParser(),
  })

  packageJsonLogger.debug({
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
  packageJsonLogger.debug({
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, EOL + stripIndents`
        ${h1('dependencies:')}
        ${ADD} bar ${versionColor('2.0.0')}
        ` + '\n')
    },
  })
})

test('groups lifecycle output', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    reportingOptions: { outputMaxWidth: 79 },
    streamParser: createStreamParser(),
  })

  const pkgId = 'registry.npmjs.org/foo/1.0.0'
  const wd = process.cwd()

  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    script: 'node foo',
    stage: 'preinstall',
    stdio: 'stdout',
    wd,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20',
    stage: 'preinstall',
    stdio: 'stdout',
    wd,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    script: 'node foo',
    stage: 'postinstall',
    stdio: 'stdout',
    wd,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo I',
    stage: 'postinstall',
    stdio: 'stdout',
    wd,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/bar/1.0.0',
    script: 'node bar',
    stage: 'postinstall',
    stdio: 'stdout',
    wd,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/bar/1.0.0',
    line: 'bar I',
    stage: 'postinstall',
    stdio: 'stdout',
    wd,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo II',
    stage: 'postinstall',
    stdio: 'stdout',
    wd,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo III',
    stage: 'postinstall',
    stdio: 'stdout',
    wd,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/qar/1.0.0',
    script: 'node qar',
    stage: 'install',
    stdio: 'stdout',
    wd,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/qar/1.0.0',
    exitCode: 0,
    stage: 'install',
    wd,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    exitCode: 0,
    stage: 'postinstall',
    wd,
  })

  t.plan(1)

  output$.skip(9).take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, EOL + stripIndents`
        registry.npmjs.org/foo/1.0.0             | ${PREINSTALL}$ node foo
        registry.npmjs.org/foo/1.0.0             | ${PREINSTALL}: foo 0 1 2 3 4 5 6 7 8 9

        registry.npmjs.org/foo/1.0.0             | ${POSTINSTALL}$ node foo
        registry.npmjs.org/foo/1.0.0             | ${POSTINSTALL}: foo I
        registry.npmjs.org/foo/1.0.0             | ${POSTINSTALL}: foo II
        registry.npmjs.org/foo/1.0.0             | ${POSTINSTALL}: foo III

        registry.npmjs.org/bar/1.0.0             | ${POSTINSTALL}$ node bar
        registry.npmjs.org/bar/1.0.0             | ${POSTINSTALL}: bar I

        registry.npmjs.org/qar/1.0.0             | ${INSTALL}$ node qar
        registry.npmjs.org/qar/1.0.0             | ${INSTALL}: done
      `)
    },
  })
})

// Many libs use stderr for logging, so showing all stderr adds not much value
test['skip']('prints lifecycle progress', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const wd = process.cwd()

  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo I',
    script: 'postinstall',
    stage: 'postinstall',
    wd,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/bar/1.0.0',
    line: 'bar I',
    script: 'postinstall',
    stage: 'postinstall',
    wd,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo II',
    script: 'postinstall',
    stage: 'postinstall',
    stdio: 'stderr',
    wd,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo III',
    script: 'postinstall',
    stage: 'postinstall',
    wd,
  })

  t.plan(1)

  const childOutputColor = chalk.grey
  const childOutputError = chalk.red

  output$.skip(3).take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        Running ${POSTINSTALL} for ${hlPkgId('registry.npmjs.org/foo/1.0.0')}: ${childOutputColor('foo I')}
        Running ${POSTINSTALL} for ${hlPkgId('registry.npmjs.org/foo/1.0.0')}! ${childOutputError('foo II')}
        Running ${POSTINSTALL} for ${hlPkgId('registry.npmjs.org/foo/1.0.0')}: ${childOutputColor('foo III')}
        Running ${POSTINSTALL} for ${hlPkgId('registry.npmjs.org/bar/1.0.0')}: ${childOutputColor('bar I')}
      `)
    },
  })
})

test('prints generic error', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const err = new Error('some error')
  logger.error(err)

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        ${ERROR} ${chalk.red('some error')}
        ${new StackTracey(err.stack).pretty}
      `)
    },
  })
})

test('prints generic error when recursive install fails', t => {
  const output$ = toOutput$({
    context: { argv: ['recursive'] },
    streamParser: createStreamParser(),
  })

  const err = new Error('some error')
  err['prefix'] = '/home/src/'
  logger.error(err, err)

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        /home/src/:
        ${ERROR} ${chalk.red('some error')}
        ${new StackTracey(err.stack).pretty}
      `)
    },
  })
})

test('prints no matching version error when many dist-tags exist', async (t) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndent`
        ${ERROR} ${chalk.red('No matching version found for pnpm@1000.0.0')}

        The latest release of pnpm is "2.4.0".

        Other releases are:
          * stable: 2.2.2
          * next: 2.4.0
          * latest-1: 1.43.1

        If you need the full list of all 281 published versions run "$ pnpm view pnpm versions".
      `)
    },
  })

  const err = new Error('No matching version found for pnpm@1000.0.0')
  err['code'] = 'ERR_PNPM_NO_MATCHING_VERSION'
  err['packageMeta'] = await loadJsonFile(path.join(__dirname, 'pnpm-meta.json'))
  logger.error(err, err)
})

test('prints no matching version error when only the latest dist-tag exists', async (t) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndent`
        ${ERROR} ${chalk.red('No matching version found for is-positive@1000.0.0')}

        The latest release of is-positive is "3.1.0".

        If you need the full list of all 4 published versions run "$ pnpm view is-positive versions".
      `)
    },
  })

  const err = new Error('No matching version found for is-positive@1000.0.0')
  err['code'] = 'ERR_PNPM_NO_MATCHING_VERSION'
  err['packageMeta'] = await loadJsonFile(path.join(__dirname, 'is-positive-meta.json'))
  logger.error(err, err)
})

test('prints summary when some packages fail', async (t) => {
  const output$ = toOutput$({
    context: { argv: ['recursive'] },
    streamParser: createStreamParser(),
  })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, EOL + stripIndent`
        Summary: ${chalk.red('6 fails')}, 7 passes

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
        ${ERROR} ${chalk.red('f failed')}
      `)
    },
  })

  const err = new Error('...')
  err['code'] = 'ERR_PNPM_RECURSIVE_FAIL'
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, 'info message')
    },
  })
})

test('prints progress of big files download', async t => {
  t.plan(6)

  let output$ = toOutput$({
    context: { argv: ['install'] },
    reportingOptions: { throttleProgress: 0 },
    streamParser: createStreamParser(),
  })
    .map(normalizeNewline) as most.Stream<string>
  const stream$: most.Stream<string>[] = []

  const pkgId1 = 'registry.npmjs.org/foo/1.0.0'
  const pkgId2 = 'registry.npmjs.org/bar/2.0.0'
  const pkgId3 = 'registry.npmjs.org/qar/3.0.0'

  stream$.push(
    output$.take(1)
      .tap(output => t.equal(output, `Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}`))
  )

  output$ = output$.skip(1)

  stream$.push(
    output$.take(1)
      .tap(output => t.equal(output, stripIndents`
        Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}
        Downloading ${hlPkgId(pkgId1)}: ${hlValue('0 B')}/${hlValue('10.5 MB')}
      `))
  )

  output$ = output$.skip(1)

  stream$.push(
    output$.take(1)
      .tap(output => t.equal(output, stripIndents`
        Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}
        Downloading ${hlPkgId(pkgId1)}: ${hlValue('5.77 MB')}/${hlValue('10.5 MB')}
      `))
  )

  output$ = output$.skip(2)

  stream$.push(
    output$.take(1)
      .tap(output => t.equal(output, stripIndents`
        Resolving: total ${hlValue('2')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}
        Downloading ${hlPkgId(pkgId1)}: ${hlValue('7.34 MB')}/${hlValue('10.5 MB')}
      `, 'downloading of small package not reported'))
  )

  output$ = output$.skip(3)

  stream$.push(
    output$.take(1)
      .tap(output => t.equal(output, stripIndents`
        Resolving: total ${hlValue('3')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}
        Downloading ${hlPkgId(pkgId1)}: ${hlValue('7.34 MB')}/${hlValue('10.5 MB')}
        Downloading ${hlPkgId(pkgId3)}: ${hlValue('19.9 MB')}/${hlValue('21 MB')}
      `))
  )

  output$ = output$.skip(1)

  stream$.push(
    output$.take(1)
      .tap(output => t.equal(output, stripIndents`
        Downloading ${hlPkgId(pkgId1)}: ${hlValue('10.5 MB')}/${hlValue('10.5 MB')}, done
        Resolving: total ${hlValue('3')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}
        Downloading ${hlPkgId(pkgId3)}: ${hlValue('19.9 MB')}/${hlValue('21 MB')}
      `))
  )

  most.mergeArray(stream$)
    .subscribe({
      complete: () => t.end(),
      error: t.end,
      next: () => undefined,
    })

  progressLogger.debug({
    pkgId: pkgId1,
    status: 'resolving_content',
  })

  progressLogger.debug({
    attempt: 1,
    pkgId: pkgId1,
    size: 1024 * 1024 * 10, // 10 MB
    status: 'fetching_started',
  })

  await delay(0)

  progressLogger.debug({
    downloaded: 1024 * 1024 * 5.5, // 5.5 MB
    pkgId: pkgId1,
    status: 'fetching_progress',
  })

  progressLogger.debug({
    pkgId: pkgId2,
    status: 'resolving_content',
  })

  progressLogger.debug({
    attempt: 1,
    pkgId: pkgId1,
    size: 10, // 10 B
    status: 'fetching_started',
  })

  progressLogger.debug({
    downloaded: 1024 * 1024 * 7,
    pkgId: pkgId1,
    status: 'fetching_progress',
  })

  progressLogger.debug({
    pkgId: pkgId3,
    status: 'resolving_content',
  })

  progressLogger.debug({
    attempt: 1,
    pkgId: pkgId3,
    size: 1024 * 1024 * 20, // 20 MB
    status: 'fetching_started',
  })

  await delay(0)

  progressLogger.debug({
    downloaded: 1024 * 1024 * 19, // 19 MB
    pkgId: pkgId3,
    status: 'fetching_progress',
  })

  progressLogger.debug({
    downloaded: 1024 * 1024 * 10, // 10 MB
    pkgId: pkgId1,
    status: 'fetching_progress',
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.green('+5')} ${chalk.red('-1')}
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.green('+2')}
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.green('+1')}
        ${ADD}`
      )
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.red('-1')}
        ${SUB}`
      )
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.green('+100')}
        ${R.repeat(ADD, 20).join('')}`
      )
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.red('-100')}
        ${R.repeat(SUB, 20).join('')}`
      )
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.green('+100')} ${chalk.red('-1')}
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.green('+1')} ${chalk.red('-100')}
        ${ADD + R.repeat(SUB, 19).join('')}`
      )
    },
  })
})

test('prints just removed during uninstallation', t => {
  const output$ = toOutput$({
    context: { argv: ['uninstall'] },
    streamParser: createStreamParser(),
  })
  const prefix = process.cwd()

  statsLogger.debug({ removed: 4, prefix })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.red('-4')}
        ${SUB + SUB + SUB + SUB}`
      )
    },
  })
})

test('prints added/removed stats and warnings during recursive installation', t => {
  const rootPrefix = '/home/jane/repo'
  const output$ = toOutput$({
    context: {
      argv: ['recursive'],
      configs: { prefix: rootPrefix } as PnpmConfigs,
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

  output$.skip(8).take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        pkg-5                                    | ${WARN} Some issue
        .                                        | ${WARN} Some other issue
        .                                        |   ${chalk.red('-1')} ${SUB}
        pkg-1                                    |   ${chalk.green('+5')}   ${chalk.red('-1')} ${ADD + SUB}
        dir/pkg-2                                | ${WARN} ${DEPRECATED} bar@2.0.0
        dir/pkg-2                                |   ${chalk.green('+2')} ${ADD}
        .../pkg-3                                |   ${chalk.green('+1')} ${ADD}
        ...ooooooooooooooooooooooooooooong-pkg-4 |   ${chalk.red('-1')} ${SUB}
        .                                        | ${WARN} ${DEPRECATED} foo@1.0.0`
      )
    },
  })
})

test('recursive installation: prints only the added stats if nothing was removed and a lot added', t => {
  const output$ = toOutput$({
    context: {
      argv: ['recursive'],
      configs: { prefix: '/home/jane/repo' } as PnpmConfigs,
    },
    reportingOptions: { outputMaxWidth: 60 },
    streamParser: createStreamParser(),
  })

  statsLogger.debug({ removed: 0, prefix: '/home/jane/repo/pkg-1' })
  statsLogger.debug({ added: 190, prefix: '/home/jane/repo/pkg-1' })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        pkg-1                                    | ${chalk.green('+190')} ${R.repeat(ADD, 12).join('')}`
      )
    },
  })
})

test('recursive installation: prints only the removed stats if nothing was added and a lot removed', t => {
  const output$ = toOutput$({
    context: {
      argv: ['recursive'],
      configs: { prefix: '/home/jane/repo' } as PnpmConfigs,
    },
    reportingOptions: { outputMaxWidth: 60 },
    streamParser: createStreamParser(),
  })

  statsLogger.debug({ removed: 190, prefix: '/home/jane/repo/pkg-1' })
  statsLogger.debug({ added: 0, prefix: '/home/jane/repo/pkg-1' })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        pkg-1                                    | ${chalk.red('-190')} ${R.repeat(SUB, 12).join('')}`
      )
    },
  })
})

test('recursive installation: prints at least one remove sign when removed !== 0', t => {
  const output$ = toOutput$({
    context: {
      argv: ['recursive'],
      configs: { prefix: '/home/jane/repo' } as PnpmConfigs,
    },
    reportingOptions: { outputMaxWidth: 62 },
    streamParser: createStreamParser(),
  })

  statsLogger.debug({ removed: 1, prefix: '/home/jane/repo/pkg-1' })
  statsLogger.debug({ added: 100, prefix: '/home/jane/repo/pkg-1' })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        pkg-1                                    | ${chalk.green('+100')}   ${chalk.red('-1')} ${R.repeat(ADD, 8).join('') + SUB}`
      )
    },
  })
})

test('recursive installation: prints at least one add sign when added !== 0', t => {
  const output$ = toOutput$({
    context: {
      argv: ['recursive'],
      configs: { prefix: '/home/jane/repo' } as PnpmConfigs,
    },
    reportingOptions: { outputMaxWidth: 62 },
    streamParser: createStreamParser(),
  })

  statsLogger.debug({ removed: 100, prefix: '/home/jane/repo/pkg-1' })
  statsLogger.debug({ added: 1, prefix: '/home/jane/repo/pkg-1' })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        pkg-1                                    |   ${chalk.green('+1')} ${chalk.red('-100')} ${ADD + R.repeat(SUB, 8).join('')}`
      )
    },
  })
})

test('recursive uninstall: prints removed packages number', t => {
  const output$ = toOutput$({
    context: {
      argv: ['recursive', 'uninstall'],
      configs: { prefix: '/home/jane/repo' } as PnpmConfigs,
    },
    reportingOptions: { outputMaxWidth: 62 },
    streamParser: createStreamParser(),
  })

  statsLogger.debug({ removed: 1, prefix: '/home/jane/repo/pkg-1' })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        pkg-1                                    |   ${chalk.red('-1')} ${SUB}`
      )
    },
  })
})

test('install: print hook message', t => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      configs: { prefix: '/home/jane/repo' } as PnpmConfigs,
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        ${chalk.magentaBright('readPackage')}: foo`
      )
    },
  })
})

test('recursive: print hook message', t => {
  const output$ = toOutput$({
    context: {
      argv: ['recursive'],
      configs: { prefix: '/home/jane/repo' } as PnpmConfigs,
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

  output$.take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, stripIndents`
        pkg-1                                    | ${chalk.magentaBright('readPackage')}: foo`
      )
    },
  })
})

test('prints skipped optional dependency info message', t => {
  const prefix = process.cwd()
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      configs: { prefix } as PnpmConfigs,
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

  output$.take(1).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `info: ${pkgId} is an optional dependency and failed compatibility check. Excluding it from installation.`)
    },
  })
})
