import { describe, expect, test } from '@jest/globals'

import { assignReportKeys } from '../src/reportKeys.js'

describe('assignReportKeys', () => {
  test('unique names are the keys', () => {
    expect(assignReportKeys([
      { name: 'pkg-a', dirKey: 'packages/a' },
      { name: 'pkg-b', dirKey: 'packages/b' },
    ])).toEqual(['pkg-a', 'pkg-b'])
  })

  test('a project without a name is keyed by its directory', () => {
    expect(assignReportKeys([
      { dirKey: 'packages/a' },
      { name: 'pkg-b', dirKey: 'packages/b' },
    ])).toEqual(['packages/a', 'pkg-b'])
  })

  test('every project sharing a name is keyed by its directory', () => {
    expect(assignReportKeys([
      { name: 'pkg-a', dirKey: 'packages/a' },
      { name: 'pkg-a', dirKey: 'packages/b' },
      { name: 'pkg-c', dirKey: 'packages/c' },
    ])).toEqual(['packages/a', 'packages/b', 'pkg-c'])
  })

  test('a name equal to a fallback directory moves to its own directory', () => {
    expect(assignReportKeys([
      { dirKey: 'tools' },
      { name: 'tools', dirKey: 'packages/tools' },
    ])).toEqual(['tools', 'packages/tools'])
  })

  test('a moved project can shadow another name', () => {
    expect(assignReportKeys([
      { dirKey: 'first' },
      { name: 'first', dirKey: 'second' },
      { name: 'second', dirKey: 'third' },
    ])).toEqual(['first', 'second', 'third'])
  })

  test('a name equal to its own directory keeps the name', () => {
    expect(assignReportKeys([
      { name: 'tools', dirKey: 'tools' },
      { name: 'pkg-b', dirKey: 'packages/b' },
    ])).toEqual(['tools', 'pkg-b'])
  })
})
