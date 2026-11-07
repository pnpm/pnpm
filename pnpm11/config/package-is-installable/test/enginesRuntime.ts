import { expect, test } from '@jest/globals'

import { getDownloadedNodeVersionFromEnginesRuntime } from '../lib/index.js'

test('intersects every runtime range alternative with the Node.js engine range', () => {
  expect(getDownloadedNodeVersionFromEnginesRuntime({
    devEngines: {
      runtime: { name: 'node', version: '^22 || ^24', onFail: 'download' },
    },
    engines: { node: '>=24' },
  })).toBe('24.0.0')
})

test('leaves named runtime selectors to the shared Node.js version', () => {
  expect(getDownloadedNodeVersionFromEnginesRuntime({
    devEngines: {
      runtime: { name: 'node', version: 'latest', onFail: 'download' },
    },
    engines: { node: '>=24' },
  })).toBeUndefined()
})
