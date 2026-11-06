import { expect, test } from '@jest/globals'
import { createStreamParser, type LogBase } from '@pnpm/logger'

import { packageIsInstallable } from '../lib/index.js'

test.each([false, true])('engine mismatch reporting for optional=%s', (optional) => {
  const logs: Array<LogBase & { name: string }> = []
  const streamParser = createStreamParser<LogBase & { name: string }>()
  const recordLog = (log: LogBase & { name: string }): void => {
    logs.push(log)
  }
  streamParser.on('data', recordLog)
  try {
    expect(packageIsInstallable('required@1.0.0', {
      name: 'required',
      version: '1.0.0',
      engines: { node: '>=999.0.0' },
    }, {
      nodeVersion: '20.0.0',
      lockfileDir: '/project',
      optional,
    })).toBe(optional ? false : null)

    const warnings = logs.filter(log => log.name === 'pnpm:install-check')
    expect(warnings).toHaveLength(optional ? 0 : 1)
    if (!optional) {
      expect(warnings[0]).toMatchObject({
        level: 'warn',
        prefix: '/project',
        message: expect.stringContaining('Unsupported engine for required@1.0.0'),
      })
    }
    expect(logs.filter(log => log.name === 'pnpm:skipped-optional-dependency')).toHaveLength(optional ? 1 : 0)
  } finally {
    streamParser.removeListener('data', recordLog)
  }
})

test.each(['engine', 'platform'])('optional %s mismatches skip without emitting a warning', (mismatch) => {
  const logs: Array<LogBase & { name: string }> = []
  const streamParser = createStreamParser<LogBase & { name: string }>()
  const recordLog = (log: LogBase & { name: string }): void => {
    logs.push(log)
  }
  streamParser.on('data', recordLog)
  try {
    expect(packageIsInstallable('optional@1.0.0', {
      name: 'optional',
      version: '1.0.0',
      ...(mismatch === 'engine' ? { engines: { node: '>=999.0.0' } } : { cpu: ['unsupported-cpu'] }),
    }, {
      nodeVersion: '20.0.0',
      lockfileDir: '/project',
      optional: true,
      engineStrict: true,
    })).toBe(false)
    expect(logs.filter(log => log.name === 'pnpm:install-check')).toHaveLength(0)
    expect(logs.filter(log => log.name === 'pnpm:skipped-optional-dependency')).toHaveLength(1)
  } finally {
    streamParser.removeListener('data', recordLog)
  }
})

test('strict engine mismatches throw without emitting a warning', () => {
  const logs: Array<LogBase & { name: string }> = []
  const streamParser = createStreamParser<LogBase & { name: string }>()
  const recordLog = (log: LogBase & { name: string }): void => {
    logs.push(log)
  }
  streamParser.on('data', recordLog)
  try {
    expect(() => packageIsInstallable('required@1.0.0', {
      name: 'required',
      version: '1.0.0',
      engines: { node: '>=999.0.0' },
    }, {
      nodeVersion: '20.0.0',
      lockfileDir: '/project',
      optional: false,
      engineStrict: true,
    })).toThrow('Unsupported engine for required@1.0.0')
    expect(logs.filter(log => log.name === 'pnpm:install-check')).toHaveLength(0)
  } finally {
    streamParser.removeListener('data', recordLog)
  }
})
