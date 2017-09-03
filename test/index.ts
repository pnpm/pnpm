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
import {toOutput$} from '../src'
import {stripIndents} from 'common-tags'
import chalk = require('chalk')

const WARN = chalk.yellow('WARN')
const ERROR = chalk.red('ERROR')
const DEPRECATED = chalk.red('deprecated')
const versionColor = chalk.grey
const ADD = chalk.green('+')
const SUB = chalk.red('-')
const h1 = chalk.blue

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
      t.equal(output, 'Resolving: total 1, reused 0, downloaded 0')
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
      t.equal(output, 'Resolving: total 1, reused 0, downloaded 1')
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

  output$.drop(3).take(1).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        ${WARN} foo
        Resolving: total 1, reused 0, downloaded 1, done
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

  output$.drop(1).take(1).subscribe({
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

test('prints lifecycle progress', t => {
  const output$ = toOutput$(createStreamParser())

  const pkgId = 'registry.npmjs.org/foo/1.0.0'

  lifecycleLogger.debug({
    pkgId: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo I',
  })
  lifecycleLogger.debug({
    pkgId: 'registry.npmjs.org/bar/1.0.0',
    line: 'bar I',
  })
  lifecycleLogger.error({
    pkgId: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo II',
  })
  lifecycleLogger.debug({
    pkgId: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo III',
  })

  t.plan(1)

  const pkgIdColor = chalk.blue
  const childOutputColor = chalk.grey

  output$.drop(3).take(1).subscribe({
    next: output => {
      t.equal(output, stripIndents`
        ${pkgIdColor('registry.npmjs.org/foo/1.0.0')}  ${childOutputColor('foo I')}
        ${pkgIdColor('registry.npmjs.org/bar/1.0.0')}  ${childOutputColor('bar I')}
        ${pkgIdColor('registry.npmjs.org/foo/1.0.0')}! ${childOutputColor('foo II')}
        ${pkgIdColor('registry.npmjs.org/foo/1.0.0')}  ${childOutputColor('foo III')}
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
      t.equal(output, `${ERROR} some error`)
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
