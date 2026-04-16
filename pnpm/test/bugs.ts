import fs from 'node:fs'

import { tempDir } from '@pnpm/prepare'

import { execPnpmSync } from './utils/index.js'

test('pnpm bugs opens bugs.url when present', async () => {
  tempDir()
  fs.writeFileSync('package.json', JSON.stringify({
    name: 'test-pkg',
    bugs: {
      url: 'https://github.com/test/pkg/issues',
    },
  }), 'utf8')

  const result = execPnpmSync(['bugs'])

  expect(result.status).toBe(0)
})

test('pnpm bugs opens bugs string URL', async () => {
  tempDir()
  fs.writeFileSync('package.json', JSON.stringify({
    name: 'test-pkg',
    bugs: 'https://github.com/test/pkg/issues',
  }), 'utf8')

  const result = execPnpmSync(['bugs'])

  expect(result.status).toBe(0)
})

test('pnpm bugs falls back to repository/issues URL', async () => {
  tempDir()
  fs.writeFileSync('package.json', JSON.stringify({
    name: 'test-pkg',
    repository: 'https://github.com/test/pkg',
  }), 'utf8')

  const result = execPnpmSync(['bugs'])

  expect(result.status).toBe(0)
})

test('pnpm bugs throws error when no bugs URL', async () => {
  tempDir()
  fs.writeFileSync('package.json', JSON.stringify({
    name: 'test-pkg',
  }), 'utf8')

  const result = execPnpmSync(['bugs'])

  expect(result.status).toBe(1)
  expect(result.stderr.toString()).toContain('bugs URL')
})
