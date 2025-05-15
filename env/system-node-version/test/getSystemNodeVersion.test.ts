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

test('getSystemNodeVersion() returns undefined if execa.sync throws an error', () => {
  // Mock execa.sync to throw an error
  (execa.sync as jest.Mock).mockImplementationOnce(() => {
    throw new Error('not found: node')
  })

  // @ts-expect-error
  process['pkg'] = {}
  expect(getSystemNodeVersionNonCached()).toBeUndefined()
  expect(execa.sync).toHaveBeenCalledWith('node', ['--version'])
})
