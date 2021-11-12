import { getSystemNodeVersionNonCached } from '@pnpm/package-is-installable/lib/getSystemNodeVersion'
import * as execa from 'execa'

jest.mock('execa', () => ({
  sync: jest.fn(() => ({
    stdout: 'v10.0.0',
  })),
}))

test('getSystemNodeVersion() executed from an executable pnpm CLI', () => {
  process['pkg'] = {}
  expect(getSystemNodeVersionNonCached()).toBe('v10.0.0')
  expect(execa.sync).toHaveBeenCalledWith('node', ['--version'])
})

test('getSystemNodeVersion() from a non-executable pnpm CLI', () => {
  delete process['pkg']
  expect(getSystemNodeVersionNonCached()).toBe(process.version)
})
