import { jest } from '@jest/globals'
import { executeTokenHelper } from '../src/executeTokenHelper.js'

test('executeTokenHelper returns stdout of the tokenHelper command', () => {
  const globalWarn = jest.fn<(message: string) => void>()
  expect(executeTokenHelper([process.execPath, '--print', '"hello world"'], { globalWarn })).toBe('hello world')
  expect(globalWarn).not.toHaveBeenCalled()
})

test('executeTokenHelper trims the output', () => {
  const globalWarn = jest.fn<(message: string) => void>()
  expect(executeTokenHelper([process.execPath, '--print', '"  hello world  \\n"'], { globalWarn })).toBe('hello world')
  expect(globalWarn).not.toHaveBeenCalled()
})

test('executeTokenHelper logs line of stderr via warnings', () => {
  const globalWarn = jest.fn<(message: string) => void>()
  expect(executeTokenHelper([process.execPath, '--eval', [
    'console.log("foo")',
    'console.error("hello")',
    'console.log("bar")',
    'console.error("world")',
  ].join('\n')], { globalWarn })).toBe('foo\nbar')
  expect(globalWarn.mock.calls).toStrictEqual([
    ['(tokenHelper stderr) hello'],
    ['(tokenHelper stderr) world'],
  ])
})

test('executeTokenHelper does not log empty stderr', () => {
  const globalWarn = jest.fn<(message: string) => void>()
  expect(executeTokenHelper([process.execPath, '--eval', [
    'console.log("foo")',
    'console.error("  ")',
    'console.log("bar")',
    'console.error()',
  ].join('\n')], { globalWarn })).toBe('foo\nbar')
  expect(globalWarn).not.toHaveBeenCalled()
})

test('executeTokenHelper rejects non-zero exit codes', () => {
  const globalWarn = jest.fn<(message: string) => void>()
  expect(() => executeTokenHelper([process.execPath, '--eval', 'process.exit(12)'], { globalWarn })).toThrow()
  expect(globalWarn).not.toHaveBeenCalled()
})
