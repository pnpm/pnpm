import { expect, test } from '@jest/globals'

import { gitSpecifiersAreEquivalent } from '../src/gitSpecifiersAreEquivalent.js'

test('recognizes equivalent git specifiers', () => {
  for (const [left, right] of [
    [
      'git://github.com/kevva/is-positive.git#97edff6',
      'git+https://github.com/kevva/is-positive.git#97edff6',
    ],
    ['git://github.com/org/lsp-mcp#main', 'git+https://github.com/org/lsp-mcp.git#main'],
    ['github:kevva/is-positive#97edff6', 'git+https://github.com/kevva/is-positive.git#97edff6'],
    ['gitlab:group/repository#main', 'git+https://gitlab.com/group/repository.git#main'],
    ['bitbucket:group/repository#main', 'git+https://bitbucket.org/group/repository.git#main'],
    ['kevva/is-positive#97edff6', 'https://github.com/kevva/is-positive.git#97edff6'],
    ['github:kevva/is-positive', 'GIT+HTTPS://github.com/kevva/is-positive.git'],
    [
      'git+https://GitHub.com/kevva/is-positive.git#97edff6',
      'git+https://github.com/kevva/is-positive.git#97edff6',
    ],
  ]) {
    expect(gitSpecifiersAreEquivalent(left, right)).toBe(true)
    expect(gitSpecifiersAreEquivalent(right, left)).toBe(true)
  }
})

test('distinguishes different git specifiers', () => {
  const canonical = 'git+https://github.com/kevva/is-positive.git#97edff6'
  for (const different of [
    'git+https://gitlab.com/kevva/is-positive.git#97edff6',
    'git+https://github.com/other/is-positive.git#97edff6',
    'git+https://github.com/kevva/other.git#97edff6',
    'git+https://github.com/kevva/is-positive.git#different',
    'git+ssh://git@github.com/kevva/is-positive.git#97edff6',
    'https://example.com/not-a-git-dependency',
    'git+https://github.com/kevva/Is-Positive.git#97edff6',
    '^1.0.0',
  ]) {
    expect(gitSpecifiersAreEquivalent(canonical, different)).toBe(false)
    expect(gitSpecifiersAreEquivalent(different, canonical)).toBe(false)
  }
})
