import { jest } from '@jest/globals'
import { executeTokenHelper, stderrLogger } from '../src/executeTokenHelper.js'

const originalWarn = stderrLogger.warn

beforeEach(() => {
  stderrLogger.warn = jest.fn(stderrLogger.warn)
})

afterEach(() => {
  (stderrLogger.warn as jest.Mock).mockRestore()
  stderrLogger.warn = originalWarn
})

test('executeTokenHelper returns stdout of the tokenHelper command', () => {
  expect(executeTokenHelper([process.execPath, '--print', '"hello world"'])).toBe('hello world')
})

test('executeTokenHelper trims the output', () => {
  expect(executeTokenHelper([process.execPath, '--print', '"  hello world  \\n"'])).toBe('hello world')
})

test('executeTokenHelper logs line of stderr via warnings', () => {
  expect(executeTokenHelper([process.execPath, '--eval', [
    'console.log("foo")',
    'console.error("hello")',
    'console.log("bar")',
    'console.error("world")',
  ].join('\n')])).toBe('foo\nbar')
  expect(
    (stderrLogger.warn as jest.Mock).mock.calls
  ).toStrictEqual([
    [{
      prefix: expect.any(String),
      message: 'tokenHelper stderr: hello',
    }],
    [{
      prefix: expect.any(String),
      message: 'tokenHelper stderr: world',
    }],
  ])
})

test('executeTokenHelper does not log empty stderr', () => {
  expect(executeTokenHelper([process.execPath, '--eval', [
    'console.log("foo")',
    'console.error("  ")',
    'console.log("bar")',
    'console.error()',
  ].join('\n')])).toBe('foo\nbar')
  expect(stderrLogger.warn).not.toHaveBeenCalled()
})

test('executeTokenHelper rejects non-zero exit codes', () => {
  expect(() => executeTokenHelper([process.execPath, '--eval', 'process.exit(12)'])).toThrow()
})
