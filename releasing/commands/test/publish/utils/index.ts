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
