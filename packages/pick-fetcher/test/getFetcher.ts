import { getFetcher } from '@pnpm/pick-fetcher'

test('should pick localTarball fetcher', () => {
  const localTarball = jest.fn()
  const fetcher = getFetcher({ localTarball }, { tarball: 'file:is-positive-1.0.0.tgz' })
  expect(fetcher).toBe(localTarball)
})

test('should pick remoteTarball fetcher', () => {
  const remoteTarball = jest.fn()
  const fetcher = getFetcher({ remoteTarball }, { tarball: 'is-positive-1.0.0.tgz' })
  expect(fetcher).toBe(remoteTarball)
})

test.each([
  'https://codeload.github.com/zkochan/is-negative/tar.gz/2fa0531ab04e300a24ef4fd7fb3a280eccb7ccc5',
  'https://bitbucket.org/pnpmjs/git-resolver/get/87cf6a67064d2ce56e8cd20624769a5512b83ff9.tar.gz',
  'https://gitlab.com/api/v4/projects/pnpm%2Fgit-resolver/repository/archive.tar.gz',
])('should pick gitHostedTarball fetcher', (tarball) => {
  const gitHostedTarball = jest.fn()
  const fetcher = getFetcher({ gitHostedTarball }, { tarball })
  expect(fetcher).toBe(gitHostedTarball)
})

test('should fail to pick fetcher if the type is not defined', () => {
  expect(() => {
    getFetcher({}, { type: 'directory' })
  }).toThrow('Fetching for dependency type "directory" is not supported')
})
