import logger, {
  createStreamParser,
} from '@pnpm/logger'
import delay = require('delay')
import test = require('tape')
import normalizeNewline = require('normalize-newline')
import {toOutput$} from 'pnpm-default-reporter'
import {stripIndents} from 'common-tags'
import chalk from 'chalk'
import most = require('most')
import StackTracey = require('stacktracey')
import R = require('ramda')

const WARN = chalk.bgYellow.black('\u2009WARN\u2009')
const ERROR = chalk.bgRed.black('\u2009ERROR\u2009')
const DEPRECATED = chalk.red('deprecated')
const versionColor = chalk.grey
const ADD = chalk.green('+')
const SUB = chalk.red('-')
const LINKED = chalk.magentaBright('#')
const h1 = chalk.blue
const hlValue = chalk.blue
const hlPkgId = chalk['whiteBright']
const POSTINSTALL = hlValue('postinstall')
const PREINSTALL = hlValue('preinstall')
const INSTALL = hlValue('install')

const progressLogger = logger<object>('progress')
const stageLogger = logger<string>('stage')
const rootLogger = logger<object>('root')
const deprecationLogger = logger<object>('deprecation')
const summaryLogger = logger<object>('summary')
const lifecycleLogger = logger<object>('lifecycle')
const packageJsonLogger = logger<object>('package-json')
const statsLogger = logger<object>('stats')

test('prints progress beginning', t => {
  const output$ = toOutput$(createStreamParser())

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
  const output$ = toOutput$(createStreamParser(), 'recursive')

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
  const output$ = toOutput$(createStreamParser())

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
  const output$ = toOutput$(createStreamParser())

  output$.skip(3).take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        ${WARN} foo
        Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('1')}, done
      `)
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

  t.plan(1)
})

test('prints "Already up-to-date"', t => {
  const output$ = toOutput$(createStreamParser())

  stageLogger.debug('resolution_done')

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
  const output$ = toOutput$(createStreamParser())

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
      t.equal(output, stripIndents`
        ${WARN} ${DEPRECATED} bar@2.0.0: This package was deprecated because bla bla bla

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
  const output$ = toOutput$(createStreamParser())

  const pkgId = 'registry.npmjs.org/foo/1.0.0'

  lifecycleLogger.debug({
    pkgId: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo',
    script: 'preinstall',
  })
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
  lifecycleLogger.debug({
    pkgId: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo II',
    script: 'postinstall',
  })
  lifecycleLogger.debug({
    pkgId: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo III',
    script: 'postinstall',
  })
  lifecycleLogger.debug({
    pkgId: 'registry.npmjs.org/qar/1.0.0',
    line: '...',
    script: 'install',
  })
  lifecycleLogger.debug({
    pkgId: 'registry.npmjs.org/qar/1.0.0',
    exitCode: 0,
    script: 'install',
  })

  t.plan(1)

  const childOutputColor = chalk.grey

  output$.skip(6).take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        Running ${PREINSTALL} for ${hlPkgId('registry.npmjs.org/foo/1.0.0')}: ${childOutputColor('foo')}
        Running ${POSTINSTALL} for ${hlPkgId('registry.npmjs.org/foo/1.0.0')}: ${childOutputColor('foo III')}
        Running ${POSTINSTALL} for ${hlPkgId('registry.npmjs.org/bar/1.0.0')}: ${childOutputColor('bar I')}
        Running ${INSTALL} for ${hlPkgId('registry.npmjs.org/qar/1.0.0')}, done
      `)
    },
    complete: () => t.end(),
    error: t.end,
  })
})

// Many libs use stderr for logging, so showing all stderr adds not much value
test['skip']('prints lifecycle progress', t => {
  const output$ = toOutput$(createStreamParser())

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
  const output$ = toOutput$(createStreamParser())

  const err = new Error('some error')
  logger.error(err)

  t.plan(1)

  output$.take(1).subscribe({
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

test('prints info', t => {
  const output$ = toOutput$(createStreamParser())

  logger.info('info message')

  t.plan(1)

  output$.take(1).subscribe({
    next: output => {
      t.equal(output, 'info message')
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints progress of big files download', async t => {
  t.plan(6)

  let output$ = toOutput$(createStreamParser()).map(normalizeNewline) as most.Stream<string>
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
        ${chalk.dim(`Downloading ${hlPkgId(pkgId1)}: ${hlValue('10.5 MB')}/${hlValue('10.5 MB')}, done`)}
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
  const output$ = toOutput$(createStreamParser(), 'install')

  statsLogger.debug({ added: 5 })
  statsLogger.debug({ removed: 1 })

  t.plan(1)

  output$.take(1).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.red('-1')} ${chalk.green('+5')}
        ${SUB}${ADD + ADD + ADD + ADD + ADD}`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints added/removed stats during installation when 0 removed', t => {
  const output$ = toOutput$(createStreamParser(), 'install')

  statsLogger.debug({ added: 2 })
  statsLogger.debug({ removed: 0 })

  t.plan(1)

  output$.take(1).subscribe({
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
  const output$ = toOutput$(createStreamParser(), 'install')

  statsLogger.debug({ removed: 0 })
  statsLogger.debug({ added: 1 })

  t.plan(1)

  output$.take(1).subscribe({
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

test('prints at least one remove sign when removed !== 0', t => {
  const output$ = toOutput$(createStreamParser(), 'install', 20)

  statsLogger.debug({ removed: 1 })
  statsLogger.debug({ added: 100 })

  t.plan(1)

  output$.take(1).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.red('-1')} ${chalk.green('+100')}
        ${SUB}${R.repeat(ADD, 19).join('')}`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints at least one add sign when added !== 0', t => {
  const output$ = toOutput$(createStreamParser(), 'install', 20)

  statsLogger.debug({ removed: 100 })
  statsLogger.debug({ added: 1 })

  t.plan(1)

  output$.take(1).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        Packages: ${chalk.red('-100')} ${chalk.green('+1')}
        ${R.repeat(SUB, 19).join('')}${ADD}`
      )
    },
    complete: () => t.end(),
    error: t.end,
  })
})

test('prints just removed during uninstallation', t => {
  const output$ = toOutput$(createStreamParser(), 'uninstall')

  statsLogger.debug({ removed: 4 })

  t.plan(1)

  output$.take(1).subscribe({
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
