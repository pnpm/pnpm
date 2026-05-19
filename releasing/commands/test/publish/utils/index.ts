import http from 'node:http'
import util from 'node:util'

import { expect } from '@jest/globals'
import { DEFAULT_OPTS as BASE_OPTS, REGISTRY_URL } from '@pnpm/testing.command-defaults'
import { safeExeca as execa } from 'execa'

export const DEFAULT_OPTS = {
  ...BASE_OPTS,
  bail: false,
}

export async function checkPkgExists (packageName: string, expectedVersion: string): Promise<void> {
  const { stdout } = await execa('pnpm', ['view', packageName, 'versions', '--registry', REGISTRY_URL, '--json'])
  const output = JSON.parse(stdout?.toString() ?? '')
  expect(Array.isArray(output) ? output[0] : output).toStrictEqual(expectedVersion)
}

export async function getPackageMetadata (packageName: string): Promise<Record<string, unknown>> {
  return new Promise((resolve, reject) => {
    const req = http.get(`${REGISTRY_URL}/${encodeURIComponent(packageName)}`, (res) => {
      let body = ''
      res.setEncoding('utf8')
      res.on('data', (chunk: string) => {
        body += chunk
      })
      res.on('end', () => {
        if (res.statusCode !== 200) {
          reject(new Error(`Failed to fetch package metadata: ${res.statusCode ?? 'unknown status'}`))
          return
        }
        try {
          resolve(JSON.parse(body) as Record<string, unknown>)
        } catch (err: unknown) {
          const message = util.types.isNativeError(err) ? err.message : String(err)
          reject(new Error(`Failed to parse package metadata response: ${message}. Response body: ${body}`))
        }
      })
    })
    req.on('error', reject)
  })
}
