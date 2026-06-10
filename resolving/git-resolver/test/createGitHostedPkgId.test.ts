import { expect, test } from '@jest/globals'
import { createGitHostedPkgId } from '@pnpm/resolving.git-resolver'

test.each([
  [{ repo: 'ssh://git@example.com/org/repo.git', commit: 'cba04669e621b85fbdb33371604de1a2898e68e9' }, 'git+ssh://git@example.com/org/repo.git#cba04669e621b85fbdb33371604de1a2898e68e9'],
  // A fully-qualified ssh URL with a port must not be rewritten by the scp
  // shorthand normalization (its `@host:2222` looks scp-like).
  [{ repo: 'ssh://git@example.com:2222/org/repo.git', commit: 'cba04669e621b85fbdb33371604de1a2898e68e9' }, 'git+ssh://git@example.com:2222/org/repo.git#cba04669e621b85fbdb33371604de1a2898e68e9'],
  [{ repo: 'git@example.com:org/repo.git', commit: 'cba04669e621b85fbdb33371604de1a2898e68e9', path: 'packages/pkg' }, 'git+ssh://git@example.com/org/repo.git#cba04669e621b85fbdb33371604de1a2898e68e9&path:packages/pkg'],
  [{ repo: 'https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git', commit: '0000000000000000000000000000000000000000' }, 'git+https://0000000000000000000000000000000000000000:x-oauth-basic@github.com/foo/bar.git#0000000000000000000000000000000000000000'],
  [{ repo: 'file:///Users/zoltan/src/pnpm/pnpm/resolving/git-resolver', commit: '988c61e11dc8d9ca0b5580cb15291951812549dc' }, 'git+file:///Users/zoltan/src/pnpm/pnpm/resolving/git-resolver#988c61e11dc8d9ca0b5580cb15291951812549dc'],
])('createGitHostedPkgId', (resolution, id) => {
  expect(createGitHostedPkgId(resolution)).toBe(id)
})
