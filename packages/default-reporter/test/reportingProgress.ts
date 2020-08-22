import { Config } from '@pnpm/config'
import {
  fetchingProgressLogger,
  progressLogger,
  stageLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import { toOutput$ } from '@pnpm/default-reporter'
import logger, {
  createStreamParser,
} from '@pnpm/logger'
import delay from 'delay'
import chalk = require('chalk')
import most = require('most')
import normalizeNewline = require('normalize-newline')
import test = require('tape')

const WARN = chalk.bgYellow.black('\u2009WARN\u2009')
const hlValue = chalk.cyanBright
const hlPkgId = chalk['whiteBright']

const EOL = '\n'

test('prints progress beginning', t => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: '/src/project' } as Config,
    },
    streamParser: createStreamParser(),
  })

  stageLogger.debug({
    prefix: '/src/project',
    stage: 'resolution_started',
  })
  progressLogger.debug({
    packageId: 'registry.npmjs.org/foo/1.0.0',
    requester: '/src/project',
    status: 'resolved',
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

test('prints progress beginning of node_modules from not cwd', t => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: '/src/projects' } as Config,
    },
    streamParser: createStreamParser(),
  })

  stageLogger.debug({
    prefix: '/src/projects/foo',
    stage: 'resolution_started',
  })
  progressLogger.debug({
    packageId: 'registry.npmjs.org/foo/1.0.0',
    requester: '/src/projects/foo',
    status: 'resolved',
  })

  t.plan(1)

  output$.take(1).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `foo                                      | Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}`)
    },
  })
})

test('prints progress beginning when appendOnly is true', t => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: '/src/project' } as Config,
    },
    reportingOptions: {
      appendOnly: true,
    },
    streamParser: createStreamParser(),
  })

  stageLogger.debug({
    prefix: '/src/project',
    stage: 'resolution_started',
  })
  progressLogger.debug({
    packageId: 'registry.npmjs.org/foo/1.0.0',
    requester: '/src/project',
    status: 'resolved',
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
    context: {
      argv: ['install'],
      config: {
        dir: '/src/project',
        recursive: true,
      } as Config,
    },
    streamParser: createStreamParser(),
  })

  stageLogger.debug({
    prefix: '/src/project',
    stage: 'resolution_started',
  })
  progressLogger.debug({
    packageId: 'registry.npmjs.org/foo/1.0.0',
    requester: '/src/project',
    status: 'resolved',
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

test('prints progress on first download', async t => {
  t.plan(1)

  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: '/src/project' } as Config,
    },
    reportingOptions: { throttleProgress: 0 },
    streamParser: createStreamParser(),
  })

  output$.skip(1).take(1).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('1')}`)
    },
  })

  const packageId = 'registry.npmjs.org/foo/1.0.0'

  stageLogger.debug({
    prefix: '/src/project',
    stage: 'resolution_started',
  })
  progressLogger.debug({
    packageId,
    requester: '/src/project',
    status: 'resolved',
  })

  await delay(10)

  progressLogger.debug({
    packageId,
    requester: '/src/project',
    status: 'fetched',
  })
})

test('moves fixed line to the end', async t => {
  t.plan(1)
  const prefix = '/src/project'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as Config,
    },
    reportingOptions: { throttleProgress: 0 },
    streamParser: createStreamParser(),
  })

  output$.skip(3).take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, `${WARN} foo` + EOL +
        `Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('1')}, done`)
    },
  })

  const packageId = 'registry.npmjs.org/foo/1.0.0'

  stageLogger.debug({
    prefix,
    stage: 'resolution_started',
  })
  progressLogger.debug({
    packageId,
    requester: prefix,
    status: 'resolved',
  })

  await delay(10)

  progressLogger.debug({
    packageId,
    requester: prefix,
    status: 'fetched',
  })
  logger.warn({ message: 'foo', prefix })

  await delay(10) // w/o delay warning goes below for some reason. Started to happen after switch to most

  stageLogger.debug({
    prefix: prefix,
    stage: 'resolution_done',
  })
  stageLogger.debug({
    prefix: prefix,
    stage: 'importing_done',
  })
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
      t.equal(output, 'Already up-to-date')
    },
  })
})

test('prints progress of big files download', async t => {
  t.plan(6)

  // eslint-disable-next-line @typescript-eslint/no-unnecessary-type-assertion
  let output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: '/src/project' } as Config,
    },
    reportingOptions: { throttleProgress: 0 },
    streamParser: createStreamParser(),
  })
    .map(normalizeNewline) as most.Stream<string>
  const stream$: Array<most.Stream<string>> = []

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
      .tap(output => t.equal(output, `\
Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}
Downloading ${hlPkgId(pkgId1)}: ${hlValue('0 B')}/${hlValue('10.5 MB')}`))
  )

  output$ = output$.skip(1)

  stream$.push(
    output$.take(1)
      .tap(output => t.equal(output, `\
Resolving: total ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}
Downloading ${hlPkgId(pkgId1)}: ${hlValue('5.77 MB')}/${hlValue('10.5 MB')}`))
  )

  output$ = output$.skip(2)

  stream$.push(
    output$.take(1)
      .tap(output => t.equal(output, `\
Resolving: total ${hlValue('2')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}
Downloading ${hlPkgId(pkgId1)}: ${hlValue('7.34 MB')}/${hlValue('10.5 MB')}`, 'downloading of small package not reported'))
  )

  output$ = output$.skip(3)

  stream$.push(
    output$.take(1)
      .tap(output => t.equal(output, `\
Resolving: total ${hlValue('3')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}
Downloading ${hlPkgId(pkgId1)}: ${hlValue('7.34 MB')}/${hlValue('10.5 MB')}
Downloading ${hlPkgId(pkgId3)}: ${hlValue('19.9 MB')}/${hlValue('21 MB')}`))
  )

  output$ = output$.skip(1)

  stream$.push(
    output$.take(1)
      .tap(output => t.equal(output, `\
Downloading ${hlPkgId(pkgId1)}: ${hlValue('10.5 MB')}/${hlValue('10.5 MB')}, done
Resolving: total ${hlValue('3')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}
Downloading ${hlPkgId(pkgId3)}: ${hlValue('19.9 MB')}/${hlValue('21 MB')}`))
  )

  most.mergeArray(stream$)
    .subscribe({
      complete: () => t.end(),
      error: t.end,
      next: () => undefined,
    })

  stageLogger.debug({
    prefix: '/src/project',
    stage: 'resolution_started',
  })

  progressLogger.debug({
    packageId: pkgId1,
    requester: '/src/project',
    status: 'resolved',
  })

  await delay(10)

  fetchingProgressLogger.debug({
    attempt: 1,
    packageId: pkgId1,
    size: 1024 * 1024 * 10, // 10 MB
    status: 'started',
  })

  await delay(10)

  fetchingProgressLogger.debug({
    downloaded: 1024 * 1024 * 5.5, // 5.5 MB
    packageId: pkgId1,
    status: 'in_progress',
  })

  progressLogger.debug({
    packageId: pkgId2,
    requester: '/src/project',
    status: 'resolved',
  })

  await delay(10)

  fetchingProgressLogger.debug({
    attempt: 1,
    packageId: pkgId1,
    size: 10, // 10 B
    status: 'started',
  })

  fetchingProgressLogger.debug({
    downloaded: 1024 * 1024 * 7,
    packageId: pkgId1,
    status: 'in_progress',
  })

  progressLogger.debug({
    packageId: pkgId3,
    requester: '/src/project',
    status: 'resolved',
  })

  fetchingProgressLogger.debug({
    attempt: 1,
    packageId: pkgId3,
    size: 1024 * 1024 * 20, // 20 MB
    status: 'started',
  })

  await delay(10)

  fetchingProgressLogger.debug({
    downloaded: 1024 * 1024 * 19, // 19 MB
    packageId: pkgId3,
    status: 'in_progress',
  })

  fetchingProgressLogger.debug({
    downloaded: 1024 * 1024 * 10, // 10 MB
    packageId: pkgId1,
    status: 'in_progress',
  })
})
