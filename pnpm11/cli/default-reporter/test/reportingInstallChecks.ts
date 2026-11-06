import { expect, test } from '@jest/globals'
import { installCheckLogger } from '@pnpm/core-loggers'
import { createStreamParser } from '@pnpm/logger'
import { firstValueFrom, from, timeout  } from 'rxjs'
import { toArray } from 'rxjs/operators'

import { toOutput$ } from '../src/index.js'
import { reportInstallChecks } from '../src/reporterForClient/reportInstallChecks.js'
import { formatWarn } from '../src/reporterForClient/utils/formatWarn.js'
import { autozoom } from '../src/reporterForClient/utils/zooming.js'
import type { ReporterPnpmConfig } from '../src/ReporterPnpmConfig.js'

test('renders the install-check message emitted by packageIsInstallable', async () => {
  const prefix = '/home/jane/project'
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { dir: prefix } as ReporterPnpmConfig,
    },
    streamParser: createStreamParser(),
  })
  const message = 'Unsupported engine for required@1.0.0: wanted: {"node":">=99"} (current: {"node":"20.10.0"})'
  installCheckLogger.warn({ message, prefix })

  expect(await firstValueFrom(output$.pipe(timeout(1000)))).toBe(formatWarn(message))
})

test('keeps legacy install-check codes and prefixes', async () => {
  const prefix = '/home/jane/project/packages/app'
  const cwd = '/home/jane/project'
  const outputs = await firstValueFrom(reportInstallChecks(from([
    { name: 'pnpm:install-check', level: 'debug', code: 'EBADPLATFORM', pkgId: 'optional@1.0.0', prefix },
    { name: 'pnpm:install-check', level: 'debug', code: 'ENOTSUP', pkgId: 'required@1.0.0', prefix, toString: () => 'legacy engine warning' },
    { name: 'pnpm:install-check', level: 'warn', prefix, message: 'Unsupported engine' },
  ]), { cwd }).pipe(toArray()))

  expect(await Promise.all(outputs.map(async output => firstValueFrom(output)))).toEqual([
    { msg: autozoom(cwd, prefix, formatWarn('Unsupported system. Skipping dependency optional@1.0.0'), { zoomOutCurrent: false }) },
    { msg: autozoom(cwd, prefix, 'legacy engine warning', { zoomOutCurrent: false }) },
    { msg: autozoom(cwd, prefix, formatWarn('Unsupported engine'), { zoomOutCurrent: false }) },
  ])
})
