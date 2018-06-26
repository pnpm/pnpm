///<reference path="../typings/index.d.ts"/>
import logger, {
  createStreamParser,
} from '@pnpm/logger'
import delay = require('delay')
import test = require('tape')
import normalizeNewline = require('normalize-newline')
import {toOutput$} from 'pnpm-default-reporter'
import {stripIndents, stripIndent} from 'common-tags'
import chalk from 'chalk'
import most = require('most')
import StackTracey = require('stacktracey')
import R = require('ramda')
import loadJsonFile = require('load-json-file')
import path = require('path')

const WARN = chalk.bgYellow.black('\u2009WARN\u2009')
const ERROR = chalk.bgRed.black('\u2009ERROR\u2009')
const DEPRECATED = chalk.red('deprecated')
const versionColor = chalk.grey
const ADD = chalk.green('+')
const SUB = chalk.red('-')
const LINKED = chalk.magentaBright('#')
const h1 = chalk.cyanBright
const hlValue = chalk.cyanBright
const hlPkgId = chalk['whiteBright']
const POSTINSTALL = hlValue('postinstall')
const PREINSTALL = hlValue(' preinstall')
const INSTALL = hlValue('    install')

const progressLogger = logger<object>('progress')
const stageLogger = logger<string>('stage')
const rootLogger = logger<object>('root')
const deprecationLogger = logger<object>('deprecation')
const summaryLogger = logger<object>('summary')
const lifecycleLogger = logger<object>('lifecycle')
const packageJsonLogger = logger<object>('package-json')
const statsLogger = logger<object>('stats')
const hookLogger = logger<object>('hook')
const skippedOptionalDependencyLogger = logger<object>('skipped-optional-dependency')
const EOL = '\n'

test('prints progress beginning', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install'})

  const pkgId = 'registry.npmjs.org/foo/1.0.0'

  progressLogger.debug({
    status: 'resolving_content',
    pkgId,
  })

  t.plan(1)

  output$.take(1).subscribe({
    next: output => {
      t.equal(output, `Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}`)
    },
    error: t.end,
    complete: () => t.end(),
  })
})

test('prints progress beginning when appendOnly is true', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install', appendOnly: true})

  const pkgId = 'registry.npmjs.org/foo/1.0.0'

  progressLogger.debug({
    status: 'resolving_content',
    pkgId,
  })

  t.plan(1)

  output$.take(1).subscribe({
    next: output => {
      t.equal(output, `Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}`)
    },
    error: t.end,
    complete: () => t.end(),
  })
})

test('prints progress beginning during recursive install', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'recursive'})

  const pkgId = 'registry.npmjs.org/foo/1.0.0'

  progressLogger.debug({
    status: 'resolving_content',
    pkgId,
  })

  t.plan(1)

  output$.take(1).subscribe({
    next: output => {
      t.equal(output, `Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}`)
    },
    error: t.end,
    complete: () => t.end(),
  })
})

test('prints progress on first download', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install', throttleProgress: 0})

  const pkgId = 'registry.npmjs.org/foo/1.0.0'

  progressLogger.debug({
    status: 'resolving_content',
    pkgId,
  })
  progressLogger.debug({
    status: 'fetched',
    pkgId,
  })

  t.plan(1)

  output$.skip(1).take(1).subscribe({
    next: output => {
      t.equal(output, `Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('1')}`)
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('moves fixed line to the end', async t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install', throttleProgress: 0})

  output$.skip(3).take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, `${WARN} foo` + EOL +
        `Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('1')}, done`)
    },
    complete: v => t.end(),
    error: t.end,
  })

  const pkgId = 'registry.npmjs.org/foo/1.0.0'

  progressLogger.debug({
    status: 'resolving_content',
    pkgId,
  })
  progressLogger.debug({
    status: 'fetched',
    pkgId,
  })
  logger.warn('foo')

  await delay(0) // w/o delay warning goes below for some reason. Started to happen after switch to most

  stageLogger.debug('resolution_done')
  stageLogger.debug('importing_done')

  t.plan(1)
})

test('prints "Already up-to-date"', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install'})

  statsLogger.debug({ added: 0 })
  statsLogger.debug({ removed: 0 })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        Already up-to-date
      `)
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints summary', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install'})

  packageJsonLogger.debug({
    initial: {
      dependencies: {
        'is-13': '^1.0.0',
      },
      devDependencies: {
        'is-negative': '^1.0.0',
      },
    },
  })
  deprecationLogger.warn({
    pkgName: 'bar',
    pkgVersion: '2.0.0',
    pkgId: 'registry.npmjs.org/bar/2.0.0',
    deprecated: 'This package was deprecated because bla bla bla',
    depth: 0,
  })
  rootLogger.info({
    added: {
      dependencyType: 'prod',
      name: 'foo',
      version: '1.0.0',
      latest: '2.0.0',
      id: 'registry.npmjs.org/foo/1.0.0',
    },
  })
  rootLogger.info({
    added: {
      dependencyType: 'prod',
      name: 'bar',
      version: '2.0.0',
      latest: '1.0.0', // this won't be printed in summary because latest is less than current version
      id: 'registry.npmjs.org/bar/2.0.0',
    },
  })
  rootLogger.info({
    removed: {
      dependencyType: 'prod',
      name: 'foo',
      version: '0.1.0',
    },
  })
  rootLogger.info({
    added: {
      dependencyType: 'dev',
      name: 'qar',
      version: '2.0.0',
      id: 'registry.npmjs.org/qar/2.0.0',
    },
  })
  rootLogger.info({
    added: {
      dependencyType: 'optional',
      name: 'lala',
      version: '1.1.0',
      id: 'registry.npmjs.org/lala/1.1.0',
    },
  })
  rootLogger.info({
    removed: {
      dependencyType: 'optional',
      name: 'is-positive',
    },
  })
  rootLogger.debug({
    linked: {
      dependencyType: 'optional',
      from: '/src/is-linked',
      name: 'is-linked',
      to: '/src/project/node_modules'
    },
  })
  rootLogger.info({
    added: {
      dependencyType: 'prod',
      name: 'winston',
      realName: 'winst0n',
      version: '1.0.0',
      latest: '1.0.0',
      id: 'registry.npmjs.org/winst0n/2.0.0',
    },
  })
  packageJsonLogger.debug({
    updated: {
      dependencies: {
        'is-negative': '^1.0.0',
      },
      devDependencies: {
        'is-13': '^1.0.0',
      },
    }
  })
  summaryLogger.info()

  t.plan(1)

  output$.skip(1).take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, `${WARN} ${DEPRECATED} bar@2.0.0: This package was deprecated because bla bla bla` + EOL + EOL +
        stripIndents`
        ${h1('dependencies:')}
        ${ADD} bar ${versionColor('2.0.0')} ${DEPRECATED}
        ${SUB} foo ${versionColor('0.1.0')}
        ${ADD} foo ${versionColor('1.0.0')} ${versionColor('(2.0.0 is available)')}
        ${SUB} is-13 ${versionColor('^1.0.0')}
        ${ADD} is-negative ${versionColor('^1.0.0')}
        ${ADD} winston <- winst0n ${versionColor('1.0.0')}

        ${h1('optionalDependencies:')}
        ${LINKED} is-linked ${chalk.magentaBright('linked from')} ${chalk.grey('/src/is-linked')}
        ${SUB} is-positive
        ${ADD} lala ${versionColor('1.1.0')}

        ${h1('devDependencies:')}
        ${ADD} is-13 ${versionColor('^1.0.0')}
        ${SUB} is-negative ${versionColor('^1.0.0')}
        ${ADD} qar ${versionColor('2.0.0')}
        ` + '\n')
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('groups lifecycle output', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install'})

  const pkgId = 'registry.npmjs.org/foo/1.0.0'

  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    script: 'node foo',
    stage: 'preinstall',
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo',
    stage: 'preinstall',
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    script: 'node foo',
    stage: 'postinstall',
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo I',
    stage: 'postinstall',
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/bar/1.0.0',
    script: 'node bar',
    stage: 'postinstall',
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/bar/1.0.0',
    line: 'bar I',
    stage: 'postinstall',
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo II',
    stage: 'postinstall',
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo III',
    stage: 'postinstall',
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/qar/1.0.0',
    script: 'node qar',
    stage: 'install',
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/qar/1.0.0',
    exitCode: 0,
    stage: 'install',
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    exitCode: 0,
    stage: 'postinstall',
  })

  t.plan(1)

  output$.skip(9).take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, EOL + stripIndents`
        registry.npmjs.org/foo/1.0.0             | ${PREINSTALL}$ node foo
        registry.npmjs.org/foo/1.0.0             | ${PREINSTALL}: foo

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
    complete: () => t.end(),
    error: t.end,
  })
})

// Many libs use stderr for logging, so showing all stderr adds not much value
test['skip']('prints lifecycle progress', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install'})

  const pkgId = 'registry.npmjs.org/foo/1.0.0'

  lifecycleLogger.debug({
    pkgId: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo I',
    script: 'postinstall',
  })
  lifecycleLogger.debug({
    pkgId: 'registry.npmjs.org/bar/1.0.0',
    line: 'bar I',
    script: 'postinstall',
  })
  lifecycleLogger.error({
    pkgId: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo II',
    script: 'postinstall',
  })
  lifecycleLogger.debug({
    pkgId: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo III',
    script: 'postinstall',
  })

  t.plan(1)

  const childOutputColor = chalk.grey
  const childOutputError = chalk.red

  output$.skip(3).take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        Running ${POSTINSTALL} for ${hlPkgId('registry.npmjs.org/foo/1.0.0')}: ${childOutputColor('foo I')}
        Running ${POSTINSTALL} for ${hlPkgId('registry.npmjs.org/foo/1.0.0')}! ${childOutputError('foo II')}
        Running ${POSTINSTALL} for ${hlPkgId('registry.npmjs.org/foo/1.0.0')}: ${childOutputColor('foo III')}
        Running ${POSTINSTALL} for ${hlPkgId('registry.npmjs.org/bar/1.0.0')}: ${childOutputColor('bar I')}
      `)
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints generic error', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install'})

  const err = new Error('some error')
  logger.error(err)

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        ${ERROR} ${chalk.red('some error')}
        ${new StackTracey(err.stack).pretty}
      `)
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints generic error when recursive install fails', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'recursive'})

  const err = new Error('some error')
  err['prefix'] = '/home/src/'
  logger.error(err, err)

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        /home/src/:
        ${ERROR} ${chalk.red('some error')}
        ${new StackTracey(err.stack).pretty}
      `)
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints no matching version error when many dist-tags exist', async (t) => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install'})

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
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
    complete: () => t.end(),
    error: t.end,
  })

  const err = new Error('No matching version found for pnpm@1000.0.0')
  err['code'] = 'ERR_PNPM_NO_MATCHING_VERSION'
  err['packageMeta'] = await loadJsonFile(path.join(__dirname, 'pnpm-meta.json'))
  logger.error(err, err)
})

test('prints no matching version error when only the latest dist-tag exists', async (t) => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install'})

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndent`
        ${ERROR} ${chalk.red('No matching version found for is-positive@1000.0.0')}

        The latest release of is-positive is "3.1.0".

        If you need the full list of all 4 published versions run "$ pnpm view is-positive versions".
      `)
    },
    complete: () => t.end(),
    error: t.end,
  })

  const err = new Error('No matching version found for is-positive@1000.0.0')
  err['code'] = 'ERR_PNPM_NO_MATCHING_VERSION'
  err['packageMeta'] = await loadJsonFile(path.join(__dirname, 'is-positive-meta.json'))
  logger.error(err, err)
})

test('prints info', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install'})

  logger.info('info message')

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, 'info message')
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints progress of big files download', async t => {
  t.plan(6)

  let output$ = toOutput$(createStreamParser(), {cmd: 'install', throttleProgress: 0})
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
      next: () => undefined,
      complete: () => t.end(),
      error: t.end,
    })

  progressLogger.debug({
    status: 'resolving_content',
    pkgId: pkgId1,
  })

  progressLogger.debug({
    status: 'fetching_started',
    pkgId: pkgId1,
    size: 1024 * 1024 * 10, // 10 MB
    attempt: 1,
  })

  await delay(0)

  progressLogger.debug({
    status: 'fetching_progress',
    pkgId: pkgId1,
    downloaded: 1024 * 1024 * 5.5, // 5.5 MB
  })

  progressLogger.debug({
    status: 'resolving_content',
    pkgId: pkgId2,
  })

  progressLogger.debug({
    status: 'fetching_started',
    pkgId: pkgId1,
    size: 10, // 10 B
    attempt: 1,
  })

  progressLogger.debug({
    status: 'fetching_progress',
    pkgId: pkgId1,
    downloaded: 1024 * 1024 * 7,
  })

  progressLogger.debug({
    status: 'resolving_content',
    pkgId: pkgId3,
  })

  progressLogger.debug({
    status: 'fetching_started',
    pkgId: pkgId3,
    size: 1024 * 1024 * 20, // 20 MB
    attempt: 1,
  })

  await delay(0)

  progressLogger.debug({
    status: 'fetching_progress',
    pkgId: pkgId3,
    downloaded: 1024 * 1024 * 19, // 19 MB
  })

  progressLogger.debug({
    status: 'fetching_progress',
    pkgId: pkgId1,
    downloaded: 1024 * 1024 * 10, // 10 MB
  })
})

test('prints added/removed stats during installation', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install'})

  statsLogger.debug({ added: 5 })
  statsLogger.debug({ removed: 1 })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.green('+5')} ${chalk.red('-1')}
        ${ADD + ADD + ADD + ADD + ADD + SUB}`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints added/removed stats during installation when 0 removed', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install'})

  statsLogger.debug({ added: 2 })
  statsLogger.debug({ removed: 0 })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.green('+2')}
        ${ADD + ADD}`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints only the added stats if nothing was removed', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install'})

  statsLogger.debug({ removed: 0 })
  statsLogger.debug({ added: 1 })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.green('+1')}
        ${ADD}`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints only the removed stats if nothing was added', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install'})

  statsLogger.debug({ removed: 1 })
  statsLogger.debug({ added: 0 })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.red('-1')}
        ${SUB}`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints only the added stats if nothing was removed and a lot added', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install', width: 20})

  statsLogger.debug({ removed: 0 })
  statsLogger.debug({ added: 100 })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.green('+100')}
        ${R.repeat(ADD, 20).join('')}`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints only the removed stats if nothing was added and a lot removed', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install', width: 20})

  statsLogger.debug({ removed: 100 })
  statsLogger.debug({ added: 0 })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.red('-100')}
        ${R.repeat(SUB, 20).join('')}`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints at least one remove sign when removed !== 0', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install', width: 20})

  statsLogger.debug({ removed: 1 })
  statsLogger.debug({ added: 100 })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.green('+100')} ${chalk.red('-1')}
        ${R.repeat(ADD, 19).join('') + SUB}`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints at least one add sign when added !== 0', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install', width: 20})

  statsLogger.debug({ removed: 100 })
  statsLogger.debug({ added: 1 })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.green('+1')} ${chalk.red('-100')}
        ${ADD + R.repeat(SUB, 19).join('')}`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints just removed during uninstallation', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'uninstall'})

  statsLogger.debug({ removed: 4 })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.red('-4')}
        ${SUB + SUB + SUB + SUB}`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints added/removed stats during recursive installation', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'recursive', cwd: '/home/jane/repo'})

  statsLogger.debug({ removed: 1, prefix: '/home/jane/repo' })
  statsLogger.debug({ added: 0, prefix: '/home/jane/repo' })
  statsLogger.debug({ removed: 0, prefix: '/home/jane/repo/pkg-5' })
  statsLogger.debug({ added: 0, prefix: '/home/jane/repo/pkg-5' })
  statsLogger.debug({ added: 2, prefix: '/home/jane/repo/dir/pkg-2' })
  statsLogger.debug({ added: 5, prefix: '/home/jane/repo/pkg-1' })
  statsLogger.debug({ removed: 1, prefix: '/home/jane/repo/pkg-1' })
  statsLogger.debug({ removed: 0, prefix: '/home/jane/repo/dir/pkg-2' })
  statsLogger.debug({ removed: 0, prefix: '/home/jane/repo/loooooooooooooooooooooooooooooooooong/pkg-3' })
  statsLogger.debug({ added: 1, prefix: '/home/jane/repo/loooooooooooooooooooooooooooooooooong/pkg-3' })
  statsLogger.debug({ removed: 1, prefix: '/home/jane/repo/loooooooooooooooooooooooooooooooooong-pkg-4' })
  statsLogger.debug({ added: 0, prefix: '/home/jane/repo/loooooooooooooooooooooooooooooooooong-pkg-4' })

  t.plan(1)

  output$.skip(4).take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        .                                        |   ${chalk.red('-1')} ${SUB}
        pkg-1                                    |   ${chalk.green('+5')}   ${chalk.red('-1')} ${ADD + SUB}
        dir/pkg-2                                |   ${chalk.green('+2')} ${ADD}
        .../pkg-3                                |   ${chalk.green('+1')} ${ADD}
        ...ooooooooooooooooooooooooooooong-pkg-4 |   ${chalk.red('-1')} ${SUB}`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('recursive installation: prints only the added stats if nothing was removed and a lot added', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'recursive', width: 60, cwd: '/home/jane/repo'})

  statsLogger.debug({ removed: 0, prefix: '/home/jane/repo/pkg-1' })
  statsLogger.debug({ added: 190, prefix: '/home/jane/repo/pkg-1' })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        pkg-1                                    | ${chalk.green('+190')} ${R.repeat(ADD, 12).join('')}`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('recursive installation: prints only the removed stats if nothing was added and a lot removed', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'recursive', width: 60, cwd: '/home/jane/repo'})

  statsLogger.debug({ removed: 190, prefix: '/home/jane/repo/pkg-1' })
  statsLogger.debug({ added: 0, prefix: '/home/jane/repo/pkg-1' })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        pkg-1                                    | ${chalk.red('-190')} ${R.repeat(SUB, 12).join('')}`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('recursive installation: prints at least one remove sign when removed !== 0', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'recursive', width: 62, cwd: '/home/jane/repo'})

  statsLogger.debug({ removed: 1, prefix: '/home/jane/repo/pkg-1' })
  statsLogger.debug({ added: 100, prefix: '/home/jane/repo/pkg-1' })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        pkg-1                                    | ${chalk.green('+100')}   ${chalk.red('-1')} ${R.repeat(ADD, 8).join('') + SUB}`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('recursive installation: prints at least one add sign when added !== 0', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'recursive', width: 62, cwd: '/home/jane/repo'})

  statsLogger.debug({ removed: 100, prefix: '/home/jane/repo/pkg-1' })
  statsLogger.debug({ added: 1, prefix: '/home/jane/repo/pkg-1' })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        pkg-1                                    |   ${chalk.green('+1')} ${chalk.red('-100')} ${ADD + R.repeat(SUB, 8).join('')}`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('recursive uninstall: prints removed packages number', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'recursive', subCmd: 'uninstall', width: 62, cwd: '/home/jane/repo'})

  statsLogger.debug({ removed: 1, prefix: '/home/jane/repo/pkg-1' })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        pkg-1                                    |   ${chalk.red('-1')} ${SUB}`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('install: print hook message', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install', cwd: '/home/jane/repo'})

  hookLogger.debug({
    from: '/home/jane/repo/pnpmfile.js',
    prefix: '/home/jane/repo',
    hook: 'readPackage',
    message: 'foo',
  })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        ${chalk.magentaBright('readPackage')}: foo`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('recursive: print hook message', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'recursive', cwd: '/home/jane/repo'})

  hookLogger.debug({
    from: '/home/jane/repo/pnpmfile.js',
    prefix: '/home/jane/repo/pkg-1',
    hook: 'readPackage',
    message: 'foo',
  })

  t.plan(1)

  output$.take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        pkg-1                                    | ${chalk.magentaBright('readPackage')}: foo`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints skipped optional dependency info message', t => {
  const output$ = toOutput$(createStreamParser(), {cmd: 'install'})

  const pkgId = 'registry.npmjs.org/foo/1.0.0'

  skippedOptionalDependencyLogger.debug({
    package: {
      id: pkgId,
      name: 'foo',
      version: '1.0.0',
    },
    parents: [],
    reason: 'unsupported_platform',
  })

  t.plan(1)

  output$.take(1).subscribe({
    next: output => {
      t.equal(output, `info: ${pkgId} is an optional dependency and failed compatibility check. Excluding it from installation.`)
    },
    error: t.end,
    complete: () => t.end(),
  })
})
