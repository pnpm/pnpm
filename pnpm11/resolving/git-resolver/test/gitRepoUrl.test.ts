import { expect, test } from '@jest/globals'
import { sshRepoUrlToHttps } from '@pnpm/resolving.git-resolver'

test.each([
  ['git@github.com:acme/widget.git', 'https://github.com/acme/widget.git'],
  ['ssh://git@github.com/acme/widget.git', 'https://github.com/acme/widget.git'],
  ['git+ssh://git@github.com/acme/widget.git', 'https://github.com/acme/widget.git'],
  // The SSH port has no meaning over HTTPS, where the host serves git on 443.
  ['ssh://git@gitlab.example.com:2222/org/repo.git', 'https://gitlab.example.com/org/repo.git'],
  ['https://github.com/acme/widget.git', undefined],
  ['git://github.com/acme/widget.git', undefined],
  ['file:///home/zoltan/src/repo', undefined],
  ['ssh://git@github.com', undefined],
])('sshRepoUrlToHttps(%s)', (repo, expected) => {
  expect(sshRepoUrlToHttps(repo)).toBe(expected)
})
