import { expect, test } from '@jest/globals'
import type { ProjectManifest } from '@pnpm/types'

import { transformRepository } from '../lib/transform/repository.js'

test('returns manifest as-is when repository is absent', () => {
  const manifest: ProjectManifest = {
    name: 'foo',
    version: '1.0.0',
  }
  expect(transformRepository(manifest)).toStrictEqual(manifest)
})

test('normalizes a string repository into the git object form', () => {
  const manifest: ProjectManifest = {
    name: 'foo',
    version: '1.0.0',
    repository: 'https://codeberg.org/tao-cumplido/binary-tools',
  }
  expect(transformRepository(manifest)).toStrictEqual({
    name: 'foo',
    version: '1.0.0',
    repository: {
      type: 'git',
      url: 'https://codeberg.org/tao-cumplido/binary-tools',
    },
  })
})

test('preserves an object repository as-is', () => {
  const manifest: ProjectManifest = {
    name: 'foo',
    version: '1.0.0',
    repository: {
      url: 'https://codeberg.org/tao-cumplido/binary-tools',
    },
  }
  expect(transformRepository(manifest)).toStrictEqual(manifest)
})
