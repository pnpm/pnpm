import { describe, expect, it } from '@jest/globals'
import { gitDownloadUrl } from '@pnpm/deps.compliance.sbom'
import type { GitResolution, TarballResolution } from '@pnpm/resolving.resolver-base'

describe('gitDownloadUrl', () => {
  it('should construct git+https URL from HTTPS repo', () => {
    const resolution: GitResolution = {
      type: 'git',
      repo: 'https://github.com/stevemao/left-pad.git',
      commit: '2fca6157fcca165438e0f9495cf0e5a4e6f71349',
    }

    expect(gitDownloadUrl(resolution)).toBe(
      'git+https://github.com/stevemao/left-pad.git#2fca6157fcca165438e0f9495cf0e5a4e6f71349'
    )
  })

  it('should construct git+ssh URL from SSH protocol repo', () => {
    const resolution: GitResolution = {
      type: 'git',
      repo: 'ssh://git@github.com/user/repo.git',
      commit: 'abc123',
    }

    expect(gitDownloadUrl(resolution)).toBe(
      'git+ssh://git@github.com/user/repo.git#abc123'
    )
  })

  it('should not add git+ prefix for SCP-style SSH URLs', () => {
    const resolution: GitResolution = {
      type: 'git',
      repo: 'git@github.com:user/repo.git',
      commit: 'abc123',
    }

    expect(gitDownloadUrl(resolution)).toBe(
      'git@github.com:user/repo.git#abc123'
    )
  })

  it('should return undefined for non-git resolutions', () => {
    const resolution: TarballResolution = {
      tarball: 'https://registry.npmjs.org/express/-/express-4.22.1.tgz',
      integrity: 'sha512-abc',
    }

    expect(gitDownloadUrl(resolution)).toBeUndefined()
  })
})
