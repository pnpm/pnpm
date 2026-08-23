import { expect, test } from '@jest/globals'
import type { PnpmError } from '@pnpm/error'
import type { ProjectManifest } from '@pnpm/types'

import { createUpdateMatching, failOnVersionsOfIndirectUpdateSpecs } from '../lib/recursive.js'

const INCLUDE_ALL = {
  dependencies: true,
  devDependencies: true,
  optionalDependencies: true,
}

const MANIFESTS: ProjectManifest[] = [{ dependencies: { foo: '^1.0.0' } }]

test('createUpdateMatching() does not match other major versions for pinned selectors', () => {
  const updateMatching = createUpdateMatching(['js-yaml@3.15.1'])

  expect(updateMatching('js-yaml', '3.15.0')).toBeTruthy()
  expect(updateMatching('js-yaml', '3.15.1')).toBeTruthy()
  expect(updateMatching('js-yaml', '4.3.0')).toBeFalsy()
})

test('createUpdateMatching() keeps 0.x selectors scoped by minor line', () => {
  const updateMatching = createUpdateMatching(['foo@0.2.5'])

  expect(updateMatching('foo', '0.2.1')).toBeTruthy()
  expect(updateMatching('foo', '0.3.0')).toBeFalsy()
  expect(updateMatching('foo', '1.0.0')).toBeFalsy()
})

test('createUpdateMatching() scopes loose exact selectors by version line', () => {
  for (const selector of ['js-yaml@v3.15.1', 'js-yaml@=3.15.1']) {
    const updateMatching = createUpdateMatching([selector])

    expect(updateMatching('js-yaml', '3.15.0')).toBeTruthy()
    expect(updateMatching('js-yaml', '4.3.0')).toBeFalsy()
  }
})

test('createUpdateMatching() evaluates all matching selectors for the same dependency', () => {
  const updateMatching = createUpdateMatching([
    'foo@npm:bar@1.0.0',
    'bar@1.0.0',
    'bar@2.0.0',
  ])

  expect(updateMatching('bar', '2.3.0')).toBeTruthy()
  expect(updateMatching('bar', '3.0.0')).toBeFalsy()
})

test('createUpdateMatching() does not apply version-line scoping for negated selectors', () => {
  const updateMatching = createUpdateMatching(['!foo@1.2.3'])

  expect(updateMatching('bar', '5.0.0')).toBeTruthy()
  expect(updateMatching('foo', '1.2.3')).toBeFalsy()
})

test('createUpdateMatching() scopes exact alias selectors by version line', () => {
  // Simulates expandUpdateSelectorsForMatching('alias@npm:pkg@100.1.0') → ['alias@npm:pkg@100.1.0', 'pkg@100.1.0']
  const updateMatching = createUpdateMatching(['alias@npm:pkg@100.1.0', 'pkg@100.1.0'])

  // 100.x versions within the requested major line are allowed
  expect(updateMatching('pkg', '100.0.0')).toBeTruthy()
  expect(updateMatching('pkg', '100.1.0')).toBeTruthy()
  expect(updateMatching('pkg', '100.2.0')).toBeTruthy()
  // 101.x is a different major line — must not leak
  expect(updateMatching('pkg', '101.0.0')).toBeFalsy()
  expect(updateMatching('pkg', '101.3.0')).toBeFalsy()
  // Unrelated packages are not matched at all
  expect(updateMatching('other-pkg', '100.0.0')).toBeFalsy()
  expect(updateMatching('other-pkg', '1.0.0')).toBeFalsy()
})

test('failOnVersionsOfIndirectUpdateSpecs() rejects an exact version nothing declares directly', () => {
  let err!: PnpmError
  try {
    failOnVersionsOfIndirectUpdateSpecs(['bar@1.2.3'], MANIFESTS, INCLUDE_ALL)
  } catch (_err: unknown) {
    err = _err as PnpmError
  }

  expect(err.code).toBe('ERR_PNPM_UPDATE_VERSION_ON_INDIRECT_DEP')
  expect(err.message).toContain('"bar" (requested "1.2.3")')
  expect(err.hint).toContain('bar@<declared range>: 1.2.3')
})

test('failOnVersionsOfIndirectUpdateSpecs() accepts a version any manifest declares directly', () => {
  expect(() => {
    failOnVersionsOfIndirectUpdateSpecs(['foo@1.2.3'], MANIFESTS, INCLUDE_ALL)
  }).not.toThrow()
})

test('failOnVersionsOfIndirectUpdateSpecs() ignores negated selectors', () => {
  // `!bar` excludes a name; the version on it requests nothing, and the
  // "everything but bar" matcher must not decide whether `bar` is direct.
  expect(() => {
    failOnVersionsOfIndirectUpdateSpecs(['!bar@1.2.3'], MANIFESTS, INCLUDE_ALL)
    failOnVersionsOfIndirectUpdateSpecs(['!bar@1.2.3'], [], INCLUDE_ALL)
  }).not.toThrow()
})

test('failOnVersionsOfIndirectUpdateSpecs() lets a range or a tag through', () => {
  for (const spec of ['bar@^1.2.3', 'bar@latest']) {
    expect(() => {
      failOnVersionsOfIndirectUpdateSpecs([spec], MANIFESTS, INCLUDE_ALL)
    }).not.toThrow()
  }
})
