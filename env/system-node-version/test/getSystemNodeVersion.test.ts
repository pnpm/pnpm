import { getSystemNodeVersionNonCached } from '../lib'
import * as execa from 'execa'

jest.mock('execa', () => ({
  sync: jest.fn(() => ({
    stdout: 'v10.0.0',
  })),
}))

test('getSystemNodeVersion() executed from an executable pnpm CLI', () => {
  // @ts-expect-error
  process['pkg'] = {}
  expect(getSystemNodeVersionNonCached()).toBe('v10.0.0')
  expect(execa.sync).toHaveBeenCalledWith('node', ['--version'])
})

test('getSystemNodeVersion() from a non-executable pnpm CLI', () => {
  // @ts-expect-error
  delete process['pkg']
  expect(getSystemNodeVersionNonCached()).toBe(process.version)
})
