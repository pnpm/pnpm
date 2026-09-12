import path from 'node:path'
import { setImmediate as tick } from 'node:timers/promises'

import { beforeEach, expect, jest, test } from '@jest/globals'
import type { ProjectManifest } from '@pnpm/types'

interface Deferred<T> {
  promise: Promise<T>
  resolve: (value: T) => void
}

interface ManifestRead {
  manifest: ProjectManifest
}

const reads = new Map<string, Deferred<ManifestRead>>()
const completionOrder: string[] = []

const tryReadProjectManifest = jest.fn(async (dependencyDir: string): Promise<ManifestRead> => {
  const name = path.basename(dependencyDir)
  const read = reads.get(name)
  if (read == null) throw new Error(`Unexpected dependency: ${name}`)
  const result = await read.promise
  completionOrder.push(name)
  return result
})

jest.unstable_mockModule('@pnpm/workspace.project-manifest-reader', () => ({
  tryReadProjectManifest,
}))

const { createExportableManifest } = await import('@pnpm/releasing.exportable-manifest')

beforeEach(() => {
  reads.clear()
  completionOrder.length = 0
  tryReadProjectManifest.mockClear()
})

test('workspace dependencies preserve declaration order when manifest reads resolve out of order', async () => {
  const first = deferred<ManifestRead>()
  const second = deferred<ManifestRead>()
  reads.set('first', first)
  reads.set('second', second)

  const resultPromise = createExportableManifest(path.resolve('project'), {
    name: 'project',
    version: '1.0.0',
    dependencies: {
      first: 'workspace:*',
      second: 'workspace:*',
    },
  }, {
    catalogs: {},
    modulesDir: path.resolve('node_modules'),
  })

  second.resolve({ manifest: { name: 'second', version: '2.0.0' } })
  await tick()
  expect(completionOrder).toStrictEqual(['second'])

  first.resolve({ manifest: { name: 'first', version: '1.0.0' } })
  const result = await resultPromise

  expect(completionOrder).toStrictEqual(['second', 'first'])
  expect(Object.keys(result.dependencies ?? {})).toStrictEqual(['first', 'second'])
  expect(result.dependencies).toStrictEqual({
    first: '1.0.0',
    second: '2.0.0',
  })
})

function deferred<T> (): Deferred<T> {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise
  })
  return { promise, resolve }
}
