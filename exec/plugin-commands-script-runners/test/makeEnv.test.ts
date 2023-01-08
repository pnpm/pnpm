import path from 'path'
import { makeEnv } from '../src/makeEnv'

test('makeEnv should fail if prependPaths has a path with a colon', () => {
  const prependPath = `/foo/bar${path.delimiter}/baz`
  expect(() => makeEnv({
    prependPaths: [prependPath],
  })).toThrow(`Cannot add ${prependPath} to PATH because it contains the path delimiter character (${path.delimiter})`)
})
