import { expect, test } from '@jest/globals'
import { sshRepoUrlToHttps } from '@pnpm/resolving.git-resolver'

test.each([
  ['git@github.com:logux/client.git', 'https://github.com/logux/client.git'],
  ['ssh://git@github.com/logux/client.git', 'https://github.com/logux/client.git'],
  ['git+ssh://git@github.com/logux/client.git', 'https://github.com/logux/client.git'],
  // The SSH port has no meaning over HTTPS, where the host serves git on 443.
  ['ssh://git@gitlab.example.com:2222/org/repo.git', 'https://gitlab.example.com/org/repo.git'],
  ['https://github.com/logux/client.git', undefined],
  ['git://github.com/logux/client.git', undefined],
  ['file:///home/zoltan/src/repo', undefined],
  ['ssh://git@github.com', undefined],
])('sshRepoUrlToHttps(%s)', (repo, expected) => {
  expect(sshRepoUrlToHttps(repo)).toBe(expected)
})
