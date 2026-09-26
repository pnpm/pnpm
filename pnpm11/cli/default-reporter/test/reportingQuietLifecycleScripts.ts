import { expect, test } from '@jest/globals'
import { lifecycleLogger } from '@pnpm/core-loggers'

import { captureOutput } from './utils/captureOutput.js'

test.each([
  ['warn', true],
  ['error', false],
] as const)('prints complete failed lifecycle output at %s level with append-only=%s', async (logLevel, appendOnly) => {
  const log = { depPath: 'packages/foo', stage: 'postinstall', wd: 'packages/foo' }
  const script = `node fail.js ${'argument'.repeat(20)}`
  const lines = Array.from({ length: 12 }, (_, index) => `line ${index + 1} ${'x'.repeat(100)}`)
  const output = await captureOutput({
    context: { argv: ['install'] },
    reportingOptions: { appendOnly, logLevel, outputMaxWidth: 40 },
  }, () => {
    lifecycleLogger.debug({ ...log, optional: false, script })
    for (const line of lines) {
      lifecycleLogger.debug({ ...log, line: `stdout ${line}`, stdio: 'stdout' })
      lifecycleLogger.debug({ ...log, line: `stderr ${line}`, stdio: 'stderr' })
    }
    lifecycleLogger.debug({ ...log, exitCode: 1, optional: false })
  })
  const rendered = appendOnly ? output : output.at(-1)?.split('\n')
  expect(rendered).toEqual([
    `packages/foo postinstall$ ${script}`,
    ...lines.flatMap((line) => [
      `packages/foo postinstall: stdout ${line}`,
      `packages/foo postinstall: stderr ${line}`,
    ]),
    'packages/foo postinstall: Failed',
  ])
})

test.each(['warn', 'error'] as const)('discards successful lifecycle output at %s level', async (logLevel) => {
  const log = { depPath: 'packages/foo', stage: 'postinstall', wd: 'packages/foo' }
  const output = await captureOutput({
    context: { argv: ['install'] },
    reportingOptions: { appendOnly: true, logLevel },
  }, () => {
    lifecycleLogger.debug({ ...log, optional: false, script: 'node success.js' })
    lifecycleLogger.debug({ ...log, line: 'successful stdout', stdio: 'stdout' })
    lifecycleLogger.debug({ ...log, line: 'successful stderr', stdio: 'stderr' })
    lifecycleLogger.debug({ ...log, exitCode: 0, optional: false })
  })
  expect(output).toEqual([])
})

test.each([
  ['warn', true],
  ['error', false],
] as const)('respects hidden lifecycle line prefixes at %s level with append-only=%s', async (logLevel, appendOnly) => {
  const log = { depPath: 'packages/foo', stage: 'postinstall', wd: 'packages/foo' }
  const output = await captureOutput({
    context: { argv: ['install'] },
    reportingOptions: { appendOnly, logLevel, hideLifecyclePrefix: true },
  }, () => {
    lifecycleLogger.debug({ ...log, optional: false, script: 'node fail.js' })
    lifecycleLogger.debug({ ...log, line: 'failed stdout', stdio: 'stdout' })
    lifecycleLogger.debug({ ...log, line: 'failed stderr', stdio: 'stderr' })
    lifecycleLogger.debug({ ...log, exitCode: 1, optional: false })
  })
  const rendered = appendOnly ? output : output.at(-1)?.split('\n')
  expect(rendered).toEqual([
    'packages/foo postinstall$ node fail.js',
    'failed stdout',
    'failed stderr',
    'packages/foo postinstall: Failed',
  ])
})

test.each(['warn', 'error'] as const)('reports optional failures only at warn and clears ignored output at %s level', async (logLevel) => {
  const log = { depPath: 'packages/foo', stage: 'postinstall', wd: 'packages/foo' }
  const output = await captureOutput({
    context: { argv: ['install'] },
    reportingOptions: { appendOnly: true, logLevel },
  }, () => {
    lifecycleLogger.debug({ ...log, optional: true, script: 'node optional.js' })
    lifecycleLogger.debug({ ...log, line: 'optional stdout', stdio: 'stdout' })
    lifecycleLogger.debug({ ...log, line: 'optional stderr', stdio: 'stderr' })
    lifecycleLogger.debug({ ...log, exitCode: 1, optional: true })
    if (logLevel === 'error') {
      lifecycleLogger.debug({ ...log, optional: false, script: 'node fatal.js' })
      lifecycleLogger.debug({ ...log, line: 'fatal stderr', stdio: 'stderr' })
      lifecycleLogger.debug({ ...log, exitCode: 1, optional: false })
    }
  })
  expect(output).toEqual(logLevel === 'warn' ? [
    'packages/foo postinstall$ node optional.js',
    'packages/foo postinstall: optional stdout',
    'packages/foo postinstall: optional stderr',
    'packages/foo postinstall: Failed (skipped as optional)',
  ] : [
    'packages/foo postinstall$ node fatal.js',
    'packages/foo postinstall: fatal stderr',
    'packages/foo postinstall: Failed',
  ])
})

test.each(['warn', 'error'] as const)('groups concurrent packages and stages and clears successful buffers at %s level', async (logLevel) => {
  const fooBuild = { depPath: 'packages/foo', stage: 'build', wd: 'packages/foo' }
  const fooPostinstall = { ...fooBuild, stage: 'postinstall' }
  const barPostinstall = { depPath: 'packages/bar', stage: 'postinstall', wd: 'packages/bar' }
  const output = await captureOutput({
    context: { argv: ['install'] },
    reportingOptions: { appendOnly: true, logLevel },
  }, () => {
    lifecycleLogger.debug({ ...fooBuild, optional: false, script: 'node build.js' })
    lifecycleLogger.debug({ ...fooBuild, line: 'build I', stdio: 'stdout' })
    lifecycleLogger.debug({ ...fooPostinstall, optional: false, script: 'node success.js' })
    lifecycleLogger.debug({ ...fooPostinstall, line: 'discarded output', stdio: 'stdout' })
    lifecycleLogger.debug({ ...barPostinstall, optional: false, script: 'node bar.js' })
    lifecycleLogger.debug({ ...barPostinstall, line: 'bar stderr', stdio: 'stderr' })
    lifecycleLogger.debug({ ...fooBuild, line: 'build II', stdio: 'stdout' })
    lifecycleLogger.debug({ ...fooPostinstall, exitCode: 0, optional: false })
    lifecycleLogger.debug({ ...barPostinstall, exitCode: 1, optional: false })
    lifecycleLogger.debug({ ...fooBuild, exitCode: 1, optional: false })
    lifecycleLogger.debug({ ...fooPostinstall, optional: false, script: 'node retry.js' })
    lifecycleLogger.debug({ ...fooPostinstall, line: 'retry stderr', stdio: 'stderr' })
    lifecycleLogger.debug({ ...fooPostinstall, exitCode: 1, optional: false })
  })
  expect(output).toEqual([
    'packages/bar postinstall$ node bar.js',
    'packages/bar postinstall: bar stderr',
    'packages/bar postinstall: Failed',
    'packages/foo build$ node build.js',
    'packages/foo build: build I',
    'packages/foo build: build II',
    'packages/foo build: Failed',
    'packages/foo postinstall$ node retry.js',
    'packages/foo postinstall: retry stderr',
    'packages/foo postinstall: Failed',
  ])
})

test.each([false, true])('retains default info optional success and failure once with aggregate-output=%s', async (aggregateOutput) => {
  const log = { depPath: 'packages/foo', stage: 'postinstall', wd: 'packages/foo' }
  const output = await captureOutput({
    context: { argv: ['install'] },
    reportingOptions: { appendOnly: true, aggregateOutput },
  }, () => {
    lifecycleLogger.debug({ ...log, optional: true, script: 'node success.js' })
    lifecycleLogger.debug({ ...log, line: 'successful output', stdio: 'stdout' })
    lifecycleLogger.debug({ ...log, exitCode: 0, optional: true })
    lifecycleLogger.debug({ ...log, optional: true, script: 'node optional.js' })
    lifecycleLogger.debug({ ...log, line: 'optional output', stdio: 'stderr' })
    lifecycleLogger.debug({ ...log, exitCode: 1, optional: true })
  })
  expect(output).toEqual([
    'packages/foo postinstall$ node success.js',
    'packages/foo postinstall: successful output',
    'packages/foo postinstall: Done',
    'packages/foo postinstall$ node optional.js',
    'packages/foo postinstall: optional output',
    'packages/foo postinstall: Failed',
  ])
})
