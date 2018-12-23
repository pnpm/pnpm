import { lifecycleLogger } from '@pnpm/core-loggers'
import { toOutput$ } from '@pnpm/default-reporter'
import { createStreamParser } from '@pnpm/logger'
import chalk from 'chalk'
import { stripIndents } from 'common-tags'
import normalizeNewline = require('normalize-newline')
import path = require('path')
import test = require('tape')

const hlValue = chalk.cyanBright
const hlPkgId = chalk['whiteBright']

const POSTINSTALL = hlValue('postinstall')
const PREINSTALL = hlValue('preinstall')
const INSTALL = hlValue('install')
const OUTPUT_INDENTATION = chalk.magentaBright('│')
const STATUS_INDENTATION = chalk.magentaBright('└─')
const STATUS_RUNNING = chalk.magentaBright('Running...')
const STATUS_DONE = chalk.magentaBright('Done in 1s')
const STATUS_FAILED = chalk.red('Failed in 1s')
const EOL = '\n'

function replaceTimeWith1Sec (text: string) {
  return text
    .replace(/Done in [a-z0-9μ]+/g, 'Done in 1s')
    .replace(/done in [a-z0-9μ]+/g, 'done in 1s')
    .replace(/Failed in [a-z0-9μ]+/g, 'Failed in 1s')
    .replace(/failed in [a-z0-9μ]+/g, 'failed in 1s')
}

test('groups lifecycle output', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    reportingOptions: { outputMaxWidth: 79 },
    streamParser: createStreamParser(),
  })

  lifecycleLogger.debug({
    depPath: 'packages/foo',
    optional: false,
    script: 'node foo',
    stage: 'preinstall',
    wd: 'packages/foo',
  })
  lifecycleLogger.debug({
    depPath: 'packages/foo',
    line: 'foo 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30',
    stage: 'preinstall',
    stdio: 'stdout',
    wd: 'packages/foo',
  })
  lifecycleLogger.debug({
    depPath: 'packages/foo',
    optional: false,
    script: 'node foo',
    stage: 'postinstall',
    wd: 'packages/foo',
  })
  lifecycleLogger.debug({
    depPath: 'packages/foo',
    line: 'foo I',
    stage: 'postinstall',
    stdio: 'stdout',
    wd: 'packages/foo',
  })
  lifecycleLogger.debug({
    depPath: 'packages/bar',
    optional: false,
    script: 'node bar',
    stage: 'postinstall',
    wd: 'packages/bar',
  })
  lifecycleLogger.debug({
    depPath: 'packages/bar',
    line: 'bar I',
    stage: 'postinstall',
    stdio: 'stdout',
    wd: 'packages/bar',
  })
  lifecycleLogger.debug({
    depPath: 'packages/foo',
    line: 'foo II',
    stage: 'postinstall',
    stdio: 'stdout',
    wd: 'packages/foo',
  })
  lifecycleLogger.debug({
    depPath: 'packages/foo',
    line: 'foo III',
    stage: 'postinstall',
    stdio: 'stdout',
    wd: 'packages/foo',
  })
  lifecycleLogger.debug({
    depPath: 'packages/qar',
    optional: false,
    script: 'node qar',
    stage: 'install',
    wd: 'packages/qar',
  })
  lifecycleLogger.debug({
    depPath: 'packages/qar',
    exitCode: 0,
    optional: false,
    stage: 'install',
    wd: 'packages/qar'
  })
  lifecycleLogger.debug({
    depPath: 'packages/foo',
    exitCode: 0,
    optional: false,
    stage: 'postinstall',
    wd: 'packages/foo'
  })

  t.plan(1)

  output$.skip(9).take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: (output: string) => {
      t.equal(replaceTimeWith1Sec(output), stripIndents`
        packages/foo ${PREINSTALL}$ node foo
        ${OUTPUT_INDENTATION} foo 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27
        ${STATUS_INDENTATION} ${STATUS_RUNNING}
        packages/foo ${POSTINSTALL}$ node foo
        ${OUTPUT_INDENTATION} foo I
        ${OUTPUT_INDENTATION} foo II
        ${OUTPUT_INDENTATION} foo III
        ${STATUS_INDENTATION} ${STATUS_RUNNING}
        packages/bar ${POSTINSTALL}$ node bar
        ${OUTPUT_INDENTATION} bar I
        ${STATUS_INDENTATION} ${STATUS_RUNNING}
        packages/qar ${INSTALL}$ node qar
        ${STATUS_INDENTATION} ${STATUS_DONE}
      `)
    },
  })
})

test('groups lifecycle output when append-only is used', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    reportingOptions: {
      appendOnly: true,
      outputMaxWidth: 79,
    },
    streamParser: createStreamParser(),
  })

  lifecycleLogger.debug({
    depPath: 'packages/foo',
    optional: false,
    script: 'node foo',
    stage: 'preinstall',
    wd: 'packages/foo',
  })
  lifecycleLogger.debug({
    depPath: 'packages/foo',
    line: 'foo 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30',
    stage: 'preinstall',
    stdio: 'stdout',
    wd: 'packages/foo',
  })
  lifecycleLogger.debug({
    depPath: 'packages/foo',
    exitCode: 1,
    optional: true,
    stage: 'preinstall',
    wd: 'packages/foo',
  })
  lifecycleLogger.debug({
    depPath: 'packages/foo',
    optional: false,
    script: 'node foo',
    stage: 'postinstall',
    wd: 'packages/foo',
  })
  lifecycleLogger.debug({
    depPath: 'packages/foo',
    line: 'foo I',
    stage: 'postinstall',
    stdio: 'stdout',
    wd: 'packages/foo',
  })
  lifecycleLogger.debug({
    depPath: 'packages/bar',
    optional: false,
    script: 'node bar',
    stage: 'postinstall',
    wd: 'packages/bar',
  })
  lifecycleLogger.debug({
    depPath: 'packages/bar',
    line: 'bar I',
    stage: 'postinstall',
    stdio: 'stdout',
    wd: 'packages/bar',
  })
  lifecycleLogger.debug({
    depPath: 'packages/foo',
    line: 'foo II',
    stage: 'postinstall',
    stdio: 'stdout',
    wd: 'packages/foo',
  })
  lifecycleLogger.debug({
    depPath: 'packages/foo',
    line: 'foo III',
    stage: 'postinstall',
    stdio: 'stdout',
    wd: 'packages/foo',
  })
  lifecycleLogger.debug({
    depPath: 'packages/qar',
    optional: false,
    script: 'node qar',
    stage: 'install',
    wd: 'packages/qar',
  })
  lifecycleLogger.debug({
    depPath: 'packages/qar',
    exitCode: 0,
    optional: false,
    stage: 'install',
    wd: 'packages/qar'
  })
  lifecycleLogger.debug({
    depPath: 'packages/foo',
    exitCode: 0,
    optional: false,
    stage: 'postinstall',
    wd: 'packages/foo'
  })

  t.plan(1)

  let allOutputs = [] as string[]

  output$.take(11).map(normalizeNewline).subscribe({
    complete: () => {
      t.equal(allOutputs.join(EOL), stripIndents`
        packages/foo ${PREINSTALL}$ node foo
        packages/foo ${PREINSTALL}: foo 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30
        packages/foo ${PREINSTALL}: Failed
        packages/foo ${POSTINSTALL}$ node foo
        packages/foo ${POSTINSTALL}: foo I
        packages/bar ${POSTINSTALL}$ node bar
        packages/bar ${POSTINSTALL}: bar I
        packages/foo ${POSTINSTALL}: foo II
        packages/foo ${POSTINSTALL}: foo III
        packages/qar ${INSTALL}$ node qar
        packages/qar ${INSTALL}: Done
      `)
      t.end()
    },
    error: t.end,
    next: (output: string) => { allOutputs.push(output) },
  })
})

test('collapse lifecycle output when it has too many lines', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    reportingOptions: { outputMaxWidth: 79 },
    streamParser: createStreamParser(),
  })

  lifecycleLogger.debug({
    depPath: 'packages/foo',
    optional: false,
    script: 'node foo',
    stage: 'postinstall',
    wd: 'packages/foo',
  })
  for (let i = 0; i < 100; i++) {
    lifecycleLogger.debug({
      depPath: 'packages/foo',
      line: `foo ${i}`,
      stage: 'postinstall',
      stdio: 'stdout',
      wd: 'packages/foo',
    })
  }
  lifecycleLogger.debug({
    depPath: 'packages/foo',
    exitCode: 0,
    optional: false,
    stage: 'postinstall',
    wd: 'packages/foo'
  })

  t.plan(1)

  output$.skip(101).take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: (output: string) => {
      t.equal(replaceTimeWith1Sec(output), stripIndents`
        packages/foo ${POSTINSTALL}$ node foo
        [90 lines collapsed]
        ${OUTPUT_INDENTATION} foo 90
        ${OUTPUT_INDENTATION} foo 91
        ${OUTPUT_INDENTATION} foo 92
        ${OUTPUT_INDENTATION} foo 93
        ${OUTPUT_INDENTATION} foo 94
        ${OUTPUT_INDENTATION} foo 95
        ${OUTPUT_INDENTATION} foo 96
        ${OUTPUT_INDENTATION} foo 97
        ${OUTPUT_INDENTATION} foo 98
        ${OUTPUT_INDENTATION} foo 99
        ${STATUS_INDENTATION} ${STATUS_DONE}
      `)
    },
  })
})

test('collapses lifecycle output of packages from node_modules', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    reportingOptions: { outputMaxWidth: 79 },
    streamParser: createStreamParser(),
  })

  const wdOfFoo = path.resolve(process.cwd(), 'node_modules', '.registry.npmjs.org', 'foo', '1.0.0', 'node_modules', 'foo')
  const wdOfBar = path.resolve(process.cwd(), 'node_modules', '.registry.npmjs.org', 'bar', '1.0.0', 'node_modules', 'bar')
  const wdOfQar = path.resolve(process.cwd(), 'node_modules', '.registry.npmjs.org', 'qar', '1.0.0', 'node_modules', 'qar')

  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    optional: false,
    script: 'node foo',
    stage: 'preinstall',
    wd: wdOfFoo,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20',
    stage: 'preinstall',
    stdio: 'stdout',
    wd: wdOfFoo,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    optional: false,
    script: 'node foo',
    stage: 'postinstall',
    stdio: 'stdout',
    wd: wdOfFoo,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo I',
    stage: 'postinstall',
    stdio: 'stdout',
    wd: wdOfFoo,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/bar/1.0.0',
    optional: false,
    script: 'node bar',
    stage: 'postinstall',
    wd: wdOfBar,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/bar/1.0.0',
    line: 'bar I',
    stage: 'postinstall',
    stdio: 'stdout',
    wd: wdOfBar,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo II',
    stage: 'postinstall',
    stdio: 'stdout',
    wd: wdOfFoo,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo III',
    stage: 'postinstall',
    stdio: 'stdout',
    wd: wdOfFoo,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/qar/1.0.0',
    optional: false,
    script: 'node qar',
    stage: 'install',
    wd: wdOfQar,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/qar/1.0.0',
    exitCode: 0,
    optional: false,
    stage: 'install',
    wd: wdOfQar,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    exitCode: 0,
    optional: false,
    stage: 'postinstall',
    wd: wdOfFoo,
  })

  t.plan(1)

  output$.skip(5).take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: (output: string) => {
      t.equal(replaceTimeWith1Sec(output), stripIndents`
        ${chalk.gray('node_modules/.registry.npmjs.org/foo/1.0.0/node_modules/')}foo: Running preinstall script...
        ${chalk.gray('node_modules/.registry.npmjs.org/foo/1.0.0/node_modules/')}foo: Running postinstall script, done in 1s
        ${chalk.gray('node_modules/.registry.npmjs.org/bar/1.0.0/node_modules/')}bar: Running postinstall script...
        ${chalk.gray('node_modules/.registry.npmjs.org/qar/1.0.0/node_modules/')}qar: Running install script, done in 1s
      `)
    },
  })
})

test('output of failed optional dependency is not shown', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    reportingOptions: { outputMaxWidth: 79 },
    streamParser: createStreamParser(),
  })

  const wd = path.resolve(process.cwd(), 'node_modules', '.registry.npmjs.org', 'foo', '1.0.0', 'node_modules', 'foo')

  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    optional: true,
    script: 'node foo',
    stage: 'install',
    wd: wd,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20',
    stage: 'install',
    stdio: 'stdout',
    wd,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    exitCode: 1,
    optional: true,
    stage: 'install',
    wd,
  })

  t.plan(1)

  output$.skip(1).take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: (output: string) => {
      t.equal(replaceTimeWith1Sec(output), stripIndents`
        ${chalk.gray('node_modules/.registry.npmjs.org/foo/1.0.0/node_modules/')}foo: Running install script, failed in 1s (skipped as optional)
      `)
    },
  })
})

test('output of failed non-optional dependency is printed', t => {
  const output$ = toOutput$({
    context: { argv: ['install'] },
    reportingOptions: { outputMaxWidth: 79 },
    streamParser: createStreamParser(),
  })

  const wd = path.resolve(process.cwd(), 'node_modules', '.registry.npmjs.org', 'foo', '1.0.0', 'node_modules', 'foo')

  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    optional: false,
    script: 'node foo',
    stage: 'install',
    wd: wd,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    line: 'foo 0 1 2 3 4 5 6 7 8 9',
    stage: 'install',
    stdio: 'stdout',
    wd,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/foo/1.0.0',
    exitCode: 1,
    optional: false,
    stage: 'install',
    wd,
  })

  t.plan(1)

  output$.skip(1).take(1).map(normalizeNewline).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: (output: string) => {
      t.equal(replaceTimeWith1Sec(output), stripIndents`
        ${chalk.gray('node_modules/.registry.npmjs.org/foo/1.0.0/node_modules/')}foo: Running install script, failed in 1s
        .../foo/1.0.0/node_modules/foo ${INSTALL}$ node foo
        ${OUTPUT_INDENTATION} foo 0 1 2 3 4 5 6 7 8 9
        ${STATUS_INDENTATION} ${STATUS_FAILED}
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
    optional: false,
    script: 'postinstall',
    stage: 'postinstall',
    wd,
  })
  lifecycleLogger.debug({
    depPath: 'registry.npmjs.org/bar/1.0.0',
    line: 'bar I',
    optional: false,
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
    optional: false,
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
