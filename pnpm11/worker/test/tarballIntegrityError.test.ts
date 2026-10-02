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
