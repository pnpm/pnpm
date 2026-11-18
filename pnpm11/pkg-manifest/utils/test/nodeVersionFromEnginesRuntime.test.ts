import { expect, test } from '@jest/globals'
import { getDownloadedNodeVersionFromEnginesRuntime, getNodeVersionFromEnginesRuntime } from '@pnpm/pkg-manifest.utils'
import type { ProjectManifest } from '@pnpm/types'

test('getNodeVersionFromEnginesRuntime() keeps the lower bound of a range pnpm downloads', () => {
  expect(getNodeVersionFromEnginesRuntime({
    devEngines: { runtime: { name: 'node', version: '>=22.12.0', onFail: 'download' } },
  })).toBe('22.12.0')
})

test('getNodeVersionFromEnginesRuntime() does not fall through from a non-downloaded range', () => {
  expect(getNodeVersionFromEnginesRuntime({
    devEngines: { runtime: { name: 'node', version: '>=22.12.0', onFail: 'error' } },
    engines: { runtime: { name: 'node', version: '20.0.0' } },
  })).toBeUndefined()
})

test('getNodeVersionFromEnginesRuntime() follows the included dependency group', () => {
  const manifest: ProjectManifest = {
    devEngines: { runtime: { name: 'node', version: '22.0.0', onFail: 'download' } },
    engines: { runtime: { name: 'node', version: '24.0.0', onFail: 'download' } },
  }
  expect(getNodeVersionFromEnginesRuntime(manifest, 'dependencies')).toBe('24.0.0')
  expect(getNodeVersionFromEnginesRuntime(manifest, 'devDependencies')).toBe('22.0.0')
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

test('getDownloadedNodeVersionFromEnginesRuntime() ignores a reified runtime dependency when runtimeOnFail disables it', () => {
  expect(getDownloadedNodeVersionFromEnginesRuntime({
    engines: {
      node: '>=99',
      runtime: { name: 'node', version: '^99.0.0', onFail: 'download' },
    },
    dependencies: { node: 'runtime:^99.0.0' },
  }, 'ignore', 'dependencies')).toBeUndefined()
})

test('getDownloadedNodeVersionFromEnginesRuntime() keeps a different direct runtime dependency when runtimeOnFail disables the engine runtime', () => {
  expect(getDownloadedNodeVersionFromEnginesRuntime({
    engines: {
      node: '>=22',
      runtime: { name: 'node', version: '^99.0.0', onFail: 'download' },
    },
    dependencies: { node: 'runtime:^22.0.0' },
  }, 'ignore', 'dependencies')).toBe('22.0.0')
})

test('getDownloadedNodeVersionFromEnginesRuntime() defers runtime aliases to resolution', () => {
  for (const version of ['latest', 'lts', 'rc/22']) {
    expect(getDownloadedNodeVersionFromEnginesRuntime({
      devEngines: { runtime: { name: 'node', version, onFail: 'download' } },
      devDependencies: { node: `runtime:${version}` },
    })).toBeUndefined()
  }
})

test('getDownloadedNodeVersionFromEnginesRuntime() falls back to engines.runtime when devEngines.runtime has no version', () => {
  expect(getDownloadedNodeVersionFromEnginesRuntime({
    devEngines: { runtime: { name: 'node', onFail: 'download' } },
    engines: { runtime: { name: 'node', version: '24.3.0', onFail: 'download' } },
  })).toBe('24.3.0')
})

test('getDownloadedNodeVersionFromEnginesRuntime() follows the runtime linked from dependencies', () => {
  expect(getDownloadedNodeVersionFromEnginesRuntime({
    devEngines: { runtime: { name: 'node', version: '22.0.0', onFail: 'download' } },
    engines: { runtime: { name: 'node', version: '24.0.0', onFail: 'download' } },
  })).toBe('24.0.0')
})

test('getDownloadedNodeVersionFromEnginesRuntime() skips a runtime that is not downloaded', () => {
  expect(getDownloadedNodeVersionFromEnginesRuntime({
    devEngines: { runtime: { name: 'node', version: '22.0.0', onFail: 'error' } },
    engines: { runtime: { name: 'node', version: '24.0.0', onFail: 'download' } },
  })).toBe('24.0.0')
  expect(getDownloadedNodeVersionFromEnginesRuntime({
    engines: { runtime: { name: 'node', version: '24.0.0', onFail: 'error' } },
    devEngines: { runtime: { name: 'node', version: '22.0.0', onFail: 'download' } },
  })).toBe('22.0.0')
})

test('getDownloadedNodeVersionFromEnginesRuntime() skips null runtime entries', () => {
  expect(getDownloadedNodeVersionFromEnginesRuntime({
    devEngines: { runtime: [null, { name: 'node', version: '24.3.0', onFail: 'download' }] },
  } as unknown as ProjectManifest)).toBe('24.3.0')
})
