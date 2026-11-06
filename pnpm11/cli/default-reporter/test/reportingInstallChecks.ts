import { expect, test } from '@jest/globals'
import { type InstallCheckLog, installCheckLogger } from '@pnpm/core-loggers'
import { createStreamParser } from '@pnpm/logger'
import { firstValueFrom, from, timeout, timer } from 'rxjs'
import { takeUntil, toArray } from 'rxjs/operators'

import { toOutput$ } from '../src/index.js'
import { type InstallCheckEvent, reportInstallChecks } from '../src/reporterForClient/reportInstallChecks.js'
import { formatWarn } from '../src/reporterForClient/utils/formatWarn.js'
import { autozoom } from '../src/reporterForClient/utils/zooming.js'
import type { ReporterPnpmConfig } from '../src/ReporterPnpmConfig.js'

test.each(['info', 'warn', 'error'] as const)('filters install-check messages at loglevel %s', async (logLevel) => {
  const prefix = '/home/jane/project'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as ReporterPnpmConfig,
    },
    streamParser: createStreamParser(),
    reportingOptions: { appendOnly: true, logLevel },
  })
  const message = 'Unsupported engine for required@1.0.0: wanted: {"node":">=99"} (current: {"node":"20.10.0"})'
  installCheckLogger.warn({ message, prefix })

  if (logLevel === 'error') {
    expect(await firstValueFrom(output$.pipe(takeUntil(timer(100)), toArray()))).toEqual([])
  } else {
    expect(await firstValueFrom(output$.pipe(timeout(1000)))).toBe(formatWarn(message))
  }
})

test('keeps legacy install-check codes and prefixes', async () => {
  const prefix = '/home/jane/project/packages/app'
  const cwd = '/home/jane/project'
  const legacyEngineLog: InstallCheckLog = {
    name: 'pnpm:install-check',
    level: 'debug',
    code: 'ENOTSUP',
    pkgId: 'required@1.0.0',
    prefix,
  }
  expect(legacyEngineLog.code.toLowerCase()).toBe('enotsup')
  expect(legacyEngineLog.pkgId.startsWith('required@')).toBe(true)
  legacyEngineLog.toString = () => 'legacy engine warning'
  const logs: InstallCheckEvent[] = [
    { name: 'pnpm:install-check', level: 'debug', code: 'EBADPLATFORM', pkgId: 'optional@1.0.0', prefix },
    legacyEngineLog,
    { name: 'pnpm:install-check', level: 'warn', prefix, message: 'Unsupported engine' },
  ]
  const outputs = await firstValueFrom(reportInstallChecks(from(logs), { cwd }).pipe(toArray()))

  expect(await Promise.all(outputs.map(async output => firstValueFrom(output)))).toEqual([
    { msg: autozoom(cwd, prefix, formatWarn('Unsupported system. Skipping dependency optional@1.0.0'), { zoomOutCurrent: false }) },
    { msg: autozoom(cwd, prefix, 'legacy engine warning', { zoomOutCurrent: false }) },
    { msg: autozoom(cwd, prefix, formatWarn('Unsupported engine'), { zoomOutCurrent: false }) },
  ])
})
