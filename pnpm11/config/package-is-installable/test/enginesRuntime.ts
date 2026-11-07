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

test('uses engines.runtime when devEngines.runtime has no version', () => {
  expect(getDownloadedNodeVersionFromEnginesRuntime({
    devEngines: {
      runtime: { name: 'node', onFail: 'download' },
    },
    engines: {
      runtime: { name: 'node', version: '24.3.0', onFail: 'download' },
      node: '>=24',
    },
  })).toBe('24.3.0')
})
