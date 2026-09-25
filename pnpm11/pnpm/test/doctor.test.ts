import http from 'node:http'
import type { AddressInfo } from 'node:net'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

import { type CheckResult, handler } from '../src/cmd/doctor.js'

test('pnpm doctor pings the configured default registry with its credentials', async () => {
  prepare()
  const authorizationHeaders: Array<string | undefined> = []
  const server = http.createServer((req, res) => {
    authorizationHeaders.push(req.headers.authorization)
    res.end('{}')
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const { port } = server.address() as AddressInfo
  const registry = `http://127.0.0.1:${port}/`
  try {
    const { output } = await handler({
      dir: process.cwd(),
      cacheDir: path.resolve('cache'),
      pnpmHomeDir: path.resolve('pnpm-home'),
      registriesByScope: { default: registry },
      configByUri: { [`//127.0.0.1:${port}/`]: { '@': { authToken: 'secret' } } },
      json: true,
      pnpmCommand: [process.execPath, '-e', ''],
    })
    const { checks } = JSON.parse(output) as { checks: CheckResult[] }
    const connectivity = checks.find((check) => check.title === 'Registry connectivity')

    expect(connectivity?.status).toBe('pass')
    expect(connectivity?.detail).toContain(registry)
    expect(authorizationHeaders).toStrictEqual(['Bearer secret'])
  } finally {
    server.close()
  }
})
