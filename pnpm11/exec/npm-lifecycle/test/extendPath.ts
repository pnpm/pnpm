import { expect, test } from '@jest/globals'

import { extendPath } from '../src/extendPath.js'

test('the path to node-gyp should be added after the path to node_modules/.bin', () => {
  const p = extendPath(process.cwd(), '', 'node_gyp', { extraBinPaths: [] })
  expect(p.indexOf('.bin')).toBeLessThan(p.indexOf('node_gyp'))
})
