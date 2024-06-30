/// <reference path="../../../__typings__/index.d.ts"/>
import path from 'path'
import { findPackages } from '@pnpm/fs.find-packages'

function compare (a: string | undefined, b: string | undefined) {
  if (a == null) return 1
  if (b == null) return -1
  return a.localeCompare(b)
}

const fixtures = path.join(__dirname, 'fixtures')

test('finds package', async () => {
  const root = path.join(fixtures, 'one-pkg')
  const pkgs = await findPackages(root)

  expect(pkgs).toHaveLength(1)
  expect(pkgs[0].rootDir).toBeDefined()
  expect(pkgs[0].manifest).toBeDefined()
})

test('finds packages by patterns', async () => {
  const root = path.join(fixtures, 'many-pkgs')
  const pkgs = await findPackages(root, { patterns: ['components/**'] })

  expect(pkgs).toHaveLength(2)
  expect(pkgs[0].rootDir).toBeDefined()
  expect(pkgs[0].manifest).toBeDefined()
  expect(pkgs[1].rootDir).toBeDefined()
  expect(pkgs[1].manifest).toBeDefined()
  expect([pkgs[0].manifest.name, pkgs[1].manifest.name].sort(compare)).toStrictEqual(['component-1', 'component-2'])
})

test('finds packages by * pattern', async () => {
  const root = path.join(fixtures, 'many-pkgs-2')
  const pkgs = await findPackages(root, { patterns: ['.', 'components/*'] })

  expect(pkgs).toHaveLength(3)
  expect([pkgs[0].manifest.name, pkgs[1].manifest.name, pkgs[2].manifest.name].sort(compare)).toStrictEqual(['component-1', 'component-2', 'many-pkgs-2'])
})

test('finds packages by default pattern', async () => {
  const root = path.join(fixtures, 'many-pkgs-2')
  const pkgs = await findPackages(root)

  expect(pkgs).toHaveLength(4)
  expect(pkgs.map(({ manifest }) => manifest.name).sort(compare)).toStrictEqual(['component-1', 'component-2', 'foo', 'many-pkgs-2'])
})

test('ignore packages by patterns', async () => {
  const root = path.join(fixtures, 'many-pkgs')
  const pkgs = await findPackages(root, { patterns: ['**', '!libs/**'] })

  expect(pkgs).toHaveLength(2)
  expect(pkgs[0].rootDir).toBeDefined()
  expect(pkgs[0].manifest).toBeDefined()
  expect(pkgs[1].rootDir).toBeDefined()
  expect(pkgs[1].manifest).toBeDefined()
  expect([pkgs[0].manifest.name, pkgs[1].manifest.name].sort(compare)).toStrictEqual(['component-1', 'component-2'])
})

test('json and yaml manifests are also found', async () => {
  const root = path.join(fixtures, 'many-pkgs-with-different-manifest-types')
  const pkgs = await findPackages(root)

  expect(pkgs).toHaveLength(3)
  expect(pkgs[0].rootDir).toBeDefined()
  expect(pkgs[0].manifest.name).toEqual('component-1')
  expect(pkgs[1].rootDir).toBeDefined()
  expect(pkgs[1].manifest.name).toEqual('component-2')
  expect(pkgs[2].rootDir).toBeDefined()
  expect(pkgs[2].manifest.name).toEqual('foo')
})
