import { getSystemNodeVersionNonCached } from '../lib/getSystemNodeVersion'
import * as execa from 'execa'

jest.mock('execa', () => ({
  sync: jest.fn(() => ({
    stdout: 'v10.0.0',
  })),
}))

const originProcess = process
let i = 0
beforeEach(() => {
  i++
  if (i === 3) {
    global.process = { ...originProcess, version: 'v21.0.0-nightly20230429c968361829' }
  }
})
afterEach(() => {
  global.process = originProcess
})

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

test('getSystemNodeVersion() a special version number', () => {
  // @ts-expect-error
  delete process['pkg']
  expect(getSystemNodeVersionNonCached()).toBe('v21.0.0')
})