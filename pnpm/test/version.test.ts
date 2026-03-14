import fs from 'fs'
import path from 'path'
import { preparePackages } from '@pnpm/prepare'
import { execPnpmSync } from './utils/index.js'

test('pnpm -r version minor should bump packages with workspace protocol dependencies without crashing', () => {
  preparePackages([
    {
      name: 'pkg-a',
      version: '1.0.0',
    },
    {
      name: 'pkg-b',
      version: '1.0.0',
      dependencies: {
        'pkg-a': 'workspace:^',
      },
    },
  ])

  fs.writeFileSync('package.json', JSON.stringify({ name: 'workspace-root', version: '1.0.0' }))
  fs.writeFileSync('pnpm-workspace.yaml', "packages:\n  - '**'\n")

  const result = execPnpmSync(['-r', 'version', 'minor'])

  if (result.status !== 0) {
    console.error('STDOUT:', result.stdout.toString())
    console.error('STDERR:', result.stderr.toString())
  }
  expect(result.status).toBe(0)

  const pkgA = JSON.parse(fs.readFileSync(path.resolve('pkg-a/package.json'), 'utf8'))
  expect(pkgA.version).toBe('1.1.0')

  const pkgB = JSON.parse(fs.readFileSync(path.resolve('pkg-b/package.json'), 'utf8'))
  expect(pkgB.version).toBe('1.1.0')
})
