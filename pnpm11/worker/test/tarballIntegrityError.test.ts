import { expect, test } from '@jest/globals'

import { TarballIntegrityError } from '../src/index.js'

test('the integrity error message hides the credentials and query of the tarball URL', () => {
  const error = new TarballIntegrityError({
    algorithm: 'sha512',
    expected: 'sha512-expected',
    found: 'sha512-found',
    sri: 'sha512-expected',
    url: 'https://user:secret@registry.example/foo/-/foo-1.0.0.tgz?token=signed#frag',
  })

  expect(error.message).toBe('Got unexpected checksum for "https://registry.example/foo/-/foo-1.0.0.tgz". Wanted "sha512-expected". Got "sha512-found".')
})

test('the integrity error message shows a local tarball path as is', () => {
  const tarball = process.platform === 'win32' ? 'C:\\project\\local-tarball.tgz' : '/project/local-tarball.tgz'
  const error = new TarballIntegrityError({
    algorithm: 'sha512',
    expected: 'sha512-expected',
    found: 'sha512-found',
    sri: 'sha512-expected',
    url: tarball,
  })

  expect(error.message).toContain(`"${tarball}"`)
})

test('the integrity error message hides the query of a tarball URL with one slash after the scheme', () => {
  const error = new TarballIntegrityError({
    algorithm: 'sha512',
    expected: 'sha512-expected',
    found: 'sha512-found',
    sri: 'sha512-expected',
    url: 'https:/registry.example/foo/-/foo-1.0.0.tgz?token=signed',
  })

  expect(error.message).not.toContain('signed')
})

test('the integrity error message strips control characters from a local tarball path', () => {
  const error = new TarballIntegrityError({
    algorithm: 'sha512',
    expected: 'sha512-expected',
    found: 'sha512-found',
    sri: 'sha512-expected',
    url: '/project/evil\n\u001b[2J/tarball.tgz',
  })

  expect(error.message).not.toContain('\n')
  expect(error.message).not.toContain('\u001b')
})
