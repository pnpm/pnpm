import test = require('tape')
import findPackages from 'find-packages'
import path = require('path')

const fixtures = path.join(__dirname, 'fixtures')

test('finds package', async t => {
  const root = path.join(fixtures, 'one-pkg')
  const pkgs = await findPackages(root)

  t.equal(pkgs.length, 1)
  t.ok(pkgs[0].path)
  t.ok(pkgs[0].manifest)
  t.end()
})

test('finds packages by patterns', async t => {
  const root = path.join(fixtures, 'many-pkgs')
  const pkgs = await findPackages(root, {patterns: ['components/**']})

  t.equal(pkgs.length, 2)
  t.ok(pkgs[0].path)
  t.ok(pkgs[0].manifest)
  t.ok(pkgs[1].path)
  t.ok(pkgs[1].manifest)
  t.deepEqual([pkgs[0].manifest.name, pkgs[1].manifest.name].sort(), ['component-1', 'component-2'])
  t.end()
})

test('ignore packages by patterns', async t => {
  const root = path.join(fixtures, 'many-pkgs')
  const pkgs = await findPackages(root, {patterns: ['**', '!libs/**']})

  t.equal(pkgs.length, 2)
  t.ok(pkgs[0].path)
  t.ok(pkgs[0].manifest)
  t.ok(pkgs[1].path)
  t.ok(pkgs[1].manifest)
  t.deepEqual([pkgs[0].manifest.name, pkgs[1].manifest.name].sort(), ['component-1', 'component-2'])
  t.end()
})

test('json and yaml manifests are also found', async t => {
  const root = path.join(fixtures, 'many-pkgs-with-different-manifest-types')
  const pkgs = await findPackages(root)

  t.equal(pkgs.length, 3)
  t.ok(pkgs[0].path)
  t.ok(pkgs[0].manifest)
  t.equal(pkgs[0].fileName, 'package.yaml')
  t.ok(pkgs[1].path)
  t.ok(pkgs[1].manifest)
  t.equal(pkgs[1].fileName, 'package.json5')
  t.ok(pkgs[2].path)
  t.ok(pkgs[2].manifest)
  t.equal(pkgs[2].fileName, 'package.json')
  t.deepEqual([pkgs[0].manifest.name, pkgs[1].manifest.name, pkgs[2].manifest.name].sort(), ['component-1', 'component-2', 'foo'])
  t.end()
})
