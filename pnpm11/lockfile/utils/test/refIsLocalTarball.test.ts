import { expect, test } from '@jest/globals'
import { refIsLocalDirectory, refIsLocalTarball } from '@pnpm/lockfile.utils'

test('refIsLocalTarball recognizes tarball extensions including bzip2', () => {
  for (const ext of ['.tgz', '.tar.gz', '.tar', '.tar.bz2', '.tbz2', '.tbz', '.TGZ', '.TAR.BZ2']) {
    expect(refIsLocalTarball(`file:../pkg${ext}`)).toBe(true)
    expect(refIsLocalDirectory(`file:../pkg${ext}`)).toBe(false)
  }

  expect(refIsLocalTarball('file:../pkg')).toBe(false)
  expect(refIsLocalDirectory('file:../pkg')).toBe(true)

  expect(refIsLocalTarball('file:../pkg.tgz(react@18.0.0)')).toBe(true)
  expect(refIsLocalDirectory('file:../pkg.tgz(react@18.0.0)')).toBe(false)
  expect(refIsLocalDirectory('file:../pkg(react@18.0.0)')).toBe(true)

  expect(refIsLocalTarball('pkg@1.0.0')).toBe(false)
  expect(refIsLocalDirectory('pkg@1.0.0')).toBe(false)
})
