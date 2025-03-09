import { type Config } from '@pnpm/config'
import {
  fetchingProgressLogger,
  progressLogger,
  stageLogger,
  statsLogger,
} from '@pnpm/core-loggers'
import { toOutput$ } from '@pnpm/default-reporter'
import {
  createStreamParser,
  logger,
} from '@pnpm/logger'
import { firstValueFrom } from 'rxjs'
import { map, skip, take, toArray } from 'rxjs/operators'
import chalk from 'chalk'
import normalizeNewline from 'normalize-newline'
import { formatWarn } from '../src/reporterForClient/utils/formatWarn'

const hlValue = chalk.cyanBright
const hlPkgId = chalk['whiteBright']

const EOL = '\n'

test('prints progress beginning', async () => {
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

  const output = await firstValueFrom(output$)
  expect(output).toBe(`Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}`)
})

test('prints progress without added packages stats', async () => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: '/src/project' } as Config,
    },
    reportingOptions: {
      hideAddedPkgsProgress: true,
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

  const output = await firstValueFrom(output$)
  expect(output).toBe(`Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}`)
})

test('prints all progress stats', async () => {
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

  const output = await firstValueFrom(output$.pipe(skip(3), take(1)))
  expect(output).toBe(`Progress: resolved ${hlValue('1')}, reused ${hlValue('1')}, downloaded ${hlValue('1')}, added ${hlValue('1')}`)
})

test('prints progress beginning of node_modules from not cwd', async () => {
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

  const output = await firstValueFrom(output$)
  expect(output).toBe(`foo                                      | Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}`)
})

test('prints progress beginning of node_modules from not cwd, when progress prefix is hidden', async () => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: '/src/projects' } as Config,
    },
    streamParser: createStreamParser(),
    reportingOptions: {
      hideProgressPrefix: true,
    },
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

  const output = await firstValueFrom(output$)
  expect(output).toBe(`Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}`)
})

test('prints progress beginning when appendOnly is true', async () => {
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

  const output = await firstValueFrom(output$)
  expect(output).toBe(`Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}`)
})

test('prints progress beginning during recursive install', async () => {
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

  const output = await firstValueFrom(output$)
  expect(output).toBe(`Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}`)
})

test('prints progress on first download', async () => {
  expect.assertions(1)

  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: '/src/project' } as Config,
    },
    reportingOptions: { throttleProgress: 0 },
    streamParser: createStreamParser(),
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

  const output = await firstValueFrom(output$.pipe(skip(1), take(1)))
  expect(output).toBe(`Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('1')}, added ${hlValue('0')}`)
})

test('moves fixed line to the end', async () => {
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
    prefix,
    stage: 'resolution_done',
  })
  stageLogger.debug({
    prefix,
    stage: 'importing_done',
  })

  const output = await firstValueFrom(output$.pipe(skip(3), take(1), map(normalizeNewline)))
  expect(output).toBe(formatWarn('foo') + EOL +
    `Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('1')}, added ${hlValue('0')}, done`)
})

test('prints "Already up to date"', async () => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    streamParser: createStreamParser(),
  })

  const prefix = process.cwd()

  statsLogger.debug({ added: 0, prefix })
  statsLogger.debug({ removed: 0, prefix })

  expect.assertions(1)

  const output = await firstValueFrom(output$.pipe(take(1), map(normalizeNewline)))
  expect(output).toBe('Already up to date')
})

test('prints progress of big files download', async () => {
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

  const output = await firstValueFrom(output$.pipe(take(9), map(normalizeNewline), toArray()))

  output.forEach((output, index) => {
    switch (index) {
    case 0:
      expect(output).toBe(`Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}`)
      return
    case 1:
      expect(output).toBe(`\
Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}
Downloading ${hlPkgId(pkgId1)}: ${hlValue('0.00 B')}/${hlValue('10.49 MB')}`)
      return
    case 2:
      expect(output).toBe(`\
Progress: resolved ${hlValue('1')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}
Downloading ${hlPkgId(pkgId1)}: ${hlValue('5.77 MB')}/${hlValue('10.49 MB')}`)
      return
    case 4:
      expect(output).toBe(`\
Progress: resolved ${hlValue('2')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}
Downloading ${hlPkgId(pkgId1)}: ${hlValue('7.34 MB')}/${hlValue('10.49 MB')}`)
      return
    case 7:
      expect(output).toBe(`\
Progress: resolved ${hlValue('3')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}
Downloading ${hlPkgId(pkgId1)}: ${hlValue('7.34 MB')}/${hlValue('10.49 MB')}
Downloading ${hlPkgId(pkgId3)}: ${hlValue('19.92 MB')}/${hlValue('20.97 MB')}`)
      return
    case 8:
      expect(output).toBe(`\
Downloading ${hlPkgId(pkgId1)}: ${hlValue('10.49 MB')}/${hlValue('10.49 MB')}, done
Progress: resolved ${hlValue('3')}, reused ${hlValue('0')}, downloaded ${hlValue('0')}, added ${hlValue('0')}
Downloading ${hlPkgId(pkgId3)}: ${hlValue('19.92 MB')}/${hlValue('20.97 MB')}`)
      return // eslint-disable-line
    }
  })
})
