import path from 'path'
import { makeEnv } from '../src/makeEnv'

test('makeEnv should fail if prependPaths has a path with a colon', () => {
  const prependPath = `/foo/bar${path.delimiter}/baz`
  expect(() => makeEnv({
    prependPaths: [prependPath],
  })).toThrow(`Cannot add ${prependPath} to PATH because it contains the path delimiter character (${path.delimiter})`)
})

test('makeEnv should exclude contributes and activationEvents properties', () => {
  const env = makeEnv({
    prependPaths: [],
    extraEnv: {
      contributes: 'some value',
      activationEvents: 'some other value',
      someOtherProperty: 'another value',
    },
  })
  expect(env.contributes).toBeUndefined()
  expect(env.activationEvents).toBeUndefined()
  expect(env.someOtherProperty).toBe('another value')
})
