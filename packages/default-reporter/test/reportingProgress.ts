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
import { map, skip, take } from 'rxjs/operators'
import chalk from 'chalk'
import normalizeNewline from 'normalize-newline'

const WARN = chalk.bgYellow.black('\u2009WARN\u2009')
const hlValue = chalk.cyanBright
const hlPkgId = chalk['whiteBright']

const EOL = '\n'

test('prints progress beginning', (done) => {
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

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}`)
    },
  })
})

test('prints all progress stats', (done) => {
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
  progressLogger.debug({
    packageId: 'registry.npmjs.org/foo/1.0.0',
    requester: '/src/project',
    status: 'fetched',
  })
  progressLogger.debug({
    packageId: 'registry.npmjs.org/bar/1.0.0',
    requester: '/src/project',
    status: 'found_in_store',
  })
  progressLogger.debug({
    method: 'hardlink',
    requester: '/src/project',
    status: 'imported',
    to: '/node_modules/.pnpm/foo@1.0.0',
  })

  expect.assertions(1)

  output$.pipe(skip(3), take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`Progress: resolved ${hlValue('1')}, reused ${hlValue('1')}, downloaded ${hlValue('1')}, added ${hlValue('1')}`)
    },
  })
})

test('prints progress beginning of node_modules from not cwd', (done) => {
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

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`foo                                      | Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}`)
    },
  })
})

test('prints progress beginning when appendOnly is true', (done) => {
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

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}`)
    },
  })
})

test('prints progress beginning during recursive install', (done) => {
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

  expect.assertions(1)

  output$.pipe(take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}`)
    },
  })
})

test('prints progress on first download', (done) => {
  expect.assertions(1)

  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: '/src/project' } as Config,
    },
    reportingOptions: { throttleProgress: 0 },
    streamParser: createStreamParser(),
  })

  output$.pipe(skip(1), take(1)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('1')}, added ${hlValue('0')}`)
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

  progressLogger.debug({
    packageId,
    requester: '/src/project',
    status: 'fetched',
  })
})

test('moves fixed line to the end', (done) => {
  expect.assertions(1)
  const prefix = '/src/project'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as Config,
    },
    reportingOptions: { throttleProgress: 0 },
    streamParser: createStreamParser(),
  })

  output$.pipe(skip(3), take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe(`${WARN} foo` + EOL +
        `Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('1')}, added ${hlValue('0')}, done`)
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

  progressLogger.debug({
    packageId,
    requester: prefix,
    status: 'fetched',
  })
  logger.warn({ message: 'foo', prefix })

  stageLogger.debug({
    prefix: prefix,
    stage: 'resolution_done',
  })
  stageLogger.debug({
    prefix: prefix,
    stage: 'importing_done',
  })
})

test('prints "Already up-to-date"', (done) => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const prefix = process.cwd()

  statsLogger.debug({ added: 0, prefix })
  statsLogger.debug({ removed: 0, prefix })

  expect.assertions(1)

  output$.pipe(take(1), map(normalizeNewline)).subscribe({
    complete: () => done(),
    error: done,
    next: output => {
      expect(output).toBe('Already up-to-date')
    },
  })
})

test('prints progress of big files download', (done) => {
  expect.assertions(6)

  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: '/src/project' } as Config,
    },
    reportingOptions: { throttleProgress: 0 },
    streamParser: createStreamParser(),
  })

  const pkgId1 = 'registry.npmjs.org/foo/1.0.0'
  const pkgId2 = 'registry.npmjs.org/bar/2.0.0'
  const pkgId3 = 'registry.npmjs.org/qar/3.0.0'

  output$.pipe(
    take(9),
    map(normalizeNewline),
    map((output, index) => {
      switch (index) {
      case 0:
        expect(output).toBe(`Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}`)
        return
      case 1:
        expect(output).toBe(`\
Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}
Downloading ${hlPkgId(pkgId1)}: ${hlValue('0 B')}/${hlValue('10.5 MB')}`)
        return
      case 2:
        expect(output).toBe(`\
Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}
Downloading ${hlPkgId(pkgId1)}: ${hlValue('5.77 MB')}/${hlValue('10.5 MB')}`)
        return
      case 4:
        expect(output).toBe(`\
Progress: resolved ${hlValue('2')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}
Downloading ${hlPkgId(pkgId1)}: ${hlValue('7.34 MB')}/${hlValue('10.5 MB')}`)
        return
      case 7:
        expect(output).toBe(`\
Progress: resolved ${hlValue('3')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}
Downloading ${hlPkgId(pkgId1)}: ${hlValue('7.34 MB')}/${hlValue('10.5 MB')}
Downloading ${hlPkgId(pkgId3)}: ${hlValue('19.9 MB')}/${hlValue('21 MB')}`)
        return
      case 8:
        expect(output).toBe(`\
Downloading ${hlPkgId(pkgId1)}: ${hlValue('10.5 MB')}/${hlValue('10.5 MB')}, done
Progress: resolved ${hlValue('3')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}
Downloading ${hlPkgId(pkgId3)}: ${hlValue('19.9 MB')}/${hlValue('21 MB')}`)
        return // eslint-disable-line
      }
    })
  )
    .subscribe({
      complete: () => done(),
      error: done,
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

  fetchingProgressLogger.debug({
    attempt: 1,
    packageId: pkgId1,
    size: 1024 * 1024 * 10, // 10 MB
    status: 'started',
  })

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
