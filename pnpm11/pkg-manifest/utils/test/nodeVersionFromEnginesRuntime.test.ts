import { expect, test } from '@jest/globals'
import { getDownloadedNodeVersionFromEnginesRuntime, getNodeVersionFromEnginesRuntime } from '@pnpm/pkg-manifest.utils'

test('getNodeVersionFromEnginesRuntime() keeps the lower bound of a range pnpm downloads', () => {
  expect(getNodeVersionFromEnginesRuntime({
    devEngines: { runtime: { name: 'node', version: '>=22.12.0', onFail: 'download' } },
  })).toBe('22.12.0')
})

test('getDownloadedNodeVersionFromEnginesRuntime() ignores a runtime pnpm does not download', () => {
  expect(getDownloadedNodeVersionFromEnginesRuntime({
    devEngines: { runtime: { name: 'node', version: '24.6.0' } },
  })).toBeUndefined()
  expect(getDownloadedNodeVersionFromEnginesRuntime({
    devEngines: { runtime: { name: 'node', version: '24.6.0', onFail: 'warn' } },
  })).toBeUndefined()
})

test('getDownloadedNodeVersionFromEnginesRuntime() follows the runtimeOnFail override', () => {
  expect(getDownloadedNodeVersionFromEnginesRuntime({
    devEngines: { runtime: { name: 'node', version: '^24.2.0', onFail: 'warn' } },
  }, 'download')).toBe('24.2.0')
  expect(getDownloadedNodeVersionFromEnginesRuntime({
    devEngines: { runtime: { name: 'node', version: '24.2.0', onFail: 'download' } },
  }, 'error')).toBeUndefined()
})

test('getDownloadedNodeVersionFromEnginesRuntime() falls back to engines.runtime when devEngines.runtime has no version', () => {
  expect(getDownloadedNodeVersionFromEnginesRuntime({
    devEngines: { runtime: { name: 'node', onFail: 'download' } },
    engines: { runtime: { name: 'node', version: '24.3.0', onFail: 'download' } },
  })).toBe('24.3.0')
})
