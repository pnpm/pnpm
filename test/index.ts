import test = require('tape')
import logger, {
  createStreamParser,
  progressLogger,
  stageLogger,
  rootLogger,
  deprecationLogger,
  summaryLogger,
  lifecycleLogger,
} from 'pnpm-logger'
import normalizeNewline = require('normalize-newline')
import {toOutput$} from '../src'
import {stripIndents} from 'common-tags'
import chalk = require('chalk')
import xs, {Stream} from 'xstream'

const WARN = chalk.bgYellow.black('\u2009WARN\u2009')
const ERROR = chalk.bgRed.black('\u2009ERROR\u2009')
const DEPRECATED = chalk.red('deprecated')
const versionColor = chalk.grey
const ADD = chalk.green('+')
const SUB = chalk.red('-')
const h1 = chalk.blue
const hlValue = chalk.blue
const hlPkgId = chalk['whiteBright']
const POSTINSTALL = hlValue('postinstall')
const PREINSTALL = hlValue('preinstall')
const INSTALL = hlValue('install')

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
    complete: t.end,
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

  output$.drop(1).take(1).subscribe({
    next: output => {
      t.equal(output, `Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('1')}`)
    },
    complete: t.end,
    error: t.end,
  })
})

test('moves fixed line to the end', t => {
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
  logger.warn('foo')
  stageLogger.debug('resolution_done')

  t.plan(1)

  output$.drop(3).take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        ${WARN} foo
        Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('1')}, done
      `)
    },
    complete: t.end,
    error: t.end,
  })
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
    complete: t.end,
    error: t.end,
  })
})

test('prints summary', t => {
  const output$ = toOutput$(createStreamParser())

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
      id: 'registry.npmjs.org/foo/1.0.0',
    },
  })
  rootLogger.info({
    added: {
      dependencyType: 'prod',
      name: 'bar',
      version: '2.0.0',
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
  summaryLogger.info(undefined)

  t.plan(1)

  output$.drop(1).take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        ${WARN} ${DEPRECATED} bar@2.0.0: This package was deprecated because bla bla bla

        ${h1('dependencies:')}
        ${ADD} bar ${versionColor('2.0.0')} ${DEPRECATED}
        ${SUB} foo ${versionColor('0.1.0')}
        ${ADD} foo ${versionColor('1.0.0')}

        ${h1('optionalDependencies:')}
        ${SUB} is-positive
        ${ADD} lala ${versionColor('1.1.0')}

        ${h1('devDependencies:')}
        ${ADD} qar ${versionColor('2.0.0')}
      ` + '\n')
    },
    complete: t.end,
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

  output$.drop(6).take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        Running ${PREINSTALL} for ${hlPkgId('registry.npmjs.org/foo/1.0.0')}: ${childOutputColor('foo')}
        Running ${POSTINSTALL} for ${hlPkgId('registry.npmjs.org/foo/1.0.0')}: ${childOutputColor('foo III')}
        Running ${POSTINSTALL} for ${hlPkgId('registry.npmjs.org/bar/1.0.0')}: ${childOutputColor('bar I')}
        Running ${INSTALL} for ${hlPkgId('registry.npmjs.org/qar/1.0.0')}, done
      `)
    },
    complete: t.end,
    error: t.end,
  })
})

test('prints lifecycle progress', t => {
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

  output$.drop(3).take(1).map(normalizeNewline).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        Running ${POSTINSTALL} for ${hlPkgId('registry.npmjs.org/foo/1.0.0')}: ${childOutputColor('foo I')}
        Running ${POSTINSTALL} for ${hlPkgId('registry.npmjs.org/foo/1.0.0')}! ${childOutputError('foo II')}
        Running ${POSTINSTALL} for ${hlPkgId('registry.npmjs.org/foo/1.0.0')}: ${childOutputColor('foo III')}
        Running ${POSTINSTALL} for ${hlPkgId('registry.npmjs.org/bar/1.0.0')}: ${childOutputColor('bar I')}
      `)
    },
    complete: t.end,
    error: t.end,
  })
})

test('prints error', t => {
  const output$ = toOutput$(createStreamParser())

  logger.error(new Error('some error'))

  t.plan(1)

  output$.take(1).subscribe({
    next: output => {
      t.equal(output, `${ERROR} ${chalk.red('some error')}`)
    },
    complete: t.end,
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
    complete: t.end,
    error: t.end,
  })
})

test('prints progress of big files download', t => {
  let output$ = toOutput$(createStreamParser()).map(normalizeNewline) as Stream<string>
  const stream$: Stream<string>[] = []

  const pkgId1 = 'registry.npmjs.org/foo/1.0.0'
  const pkgId2 = 'registry.npmjs.org/bar/2.0.0'
  const pkgId3 = 'registry.npmjs.org/qar/3.0.0'

  progressLogger.debug({
    status: 'resolving_content',
    pkgId: pkgId1,
  })

  stream$.push(
    output$.take(1)
      .debug(output => t.equal(output, `Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}`))
  )

  output$ = output$.drop(1)

  progressLogger.debug({
    status: 'fetching_started',
    pkgId: pkgId1,
    size: 1024 * 1024 * 10, // 10 MB
    attempt: 1,
  })

  stream$.push(
    output$.take(1)
      .debug(output => t.equal(output, stripIndents`
        Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}
        Downloading ${hlPkgId(pkgId1)}: ${hlValue('0 B')}/${hlValue('10.5 MB')}
      `))
  )

  output$ = output$.drop(1)

  progressLogger.debug({
    status: 'fetching_progress',
    pkgId: pkgId1,
    downloaded: 1024 * 1024 * 5.5, // 5.5 MB
  })

  stream$.push(
    output$.take(1)
      .debug(output => t.equal(output, stripIndents`
        Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}
        Downloading ${hlPkgId(pkgId1)}: ${hlValue('5.77 MB')}/${hlValue('10.5 MB')}
      `))
  )

  output$ = output$.drop(1)

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

  stream$.push(
    output$.drop(1).take(1)
      .debug(output => t.equal(output, stripIndents`
        Resolving: total ${hlValue('2')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}
        Downloading ${hlPkgId(pkgId1)}: ${hlValue('7.34 MB')}/${hlValue('10.5 MB')}
      `, 'downloading of small package not reported'))
  )

  output$ = output$.drop(2)

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

  progressLogger.debug({
    status: 'fetching_progress',
    pkgId: pkgId3,
    downloaded: 1024 * 1024 * 19, // 19 MB
  })

  stream$.push(
    output$.drop(2).take(1)
      .debug(output => t.equal(output, stripIndents`
        Resolving: total ${hlValue('3')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}
        Downloading ${hlPkgId(pkgId1)}: ${hlValue('7.34 MB')}/${hlValue('10.5 MB')}
        Downloading ${hlPkgId(pkgId3)}: ${hlValue('19.9 MB')}/${hlValue('21 MB')}
      `))
  )

  output$ = output$.drop(3)

  progressLogger.debug({
    status: 'fetching_progress',
    pkgId: pkgId1,
    downloaded: 1024 * 1024 * 10, // 10 MB
  })

  stream$.push(
    output$.take(1)
      .debug(output => t.equal(output, stripIndents`
        ${chalk.dim(`Downloading ${hlPkgId(pkgId1)}: ${hlValue('10.5 MB')}/${hlValue('10.5 MB')}, done`)}
        Resolving: total ${hlValue('3')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}
        Downloading ${hlPkgId(pkgId3)}: ${hlValue('19.9 MB')}/${hlValue('21 MB')}
      `))
  )

  xs.combine
    .apply(xs, stream$)
    .subscribe({
      complete: t.end,
      error: t.end,
    })
})
