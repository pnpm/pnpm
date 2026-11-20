import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { findPackages, findPackagesSync } from '@pnpm/workspace.projects-reader'
import { temporaryDirectory } from 'tempy'

test.each([
  ['async', findPackages],
  ['sync', findPackagesSync],
] as const)('%s discovery returns one project per directory in manifest precedence order', async (_mode, find) => {
  const root = temporaryDirectory()
  try {
    for (const dir of ['all', 'json5-and-yaml', 'yaml-only']) {
      fs.mkdirSync(path.join(root, dir))
      fs.writeFileSync(path.join(root, dir, 'package.yaml'), 'name: from-yaml\n')
    }
    for (const dir of ['all', 'json5-and-yaml']) {
      fs.writeFileSync(path.join(root, dir, 'package.json5'), "{ name: 'from-json5', }\n")
    }
    fs.writeFileSync(path.join(root, 'all/package.json'), '{"name":"from-json"}\n')

    const projects = await find(root, { patterns: ['*'], includeRoot: true })
    expect(projects.map(({ rootDir, manifest }) => [path.basename(rootDir), manifest.name])).toStrictEqual([
      ['all', 'from-json'],
      ['json5-and-yaml', 'from-json5'],
      ['yaml-only', 'from-yaml'],
    ])

    const originalJson5 = fs.readFileSync(path.join(root, 'all/package.json5'), 'utf8')
    await projects[0].writeProjectManifest({ ...projects[0].manifest, version: '1.2.3' })
    expect(JSON.parse(fs.readFileSync(path.join(root, 'all/package.json'), 'utf8')).version).toBe('1.2.3')
    expect(fs.readFileSync(path.join(root, 'all/package.json5'), 'utf8')).toBe(originalJson5)
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test.each([
  ['async', findPackages],
  ['sync', findPackagesSync],
] as const)('%s discovery does not parse an unselected manifest', async (_mode, find) => {
  const root = temporaryDirectory()
  try {
    fs.writeFileSync(path.join(root, 'package.json'), '{"name":"from-json"}\n')
    fs.writeFileSync(path.join(root, 'package.json5'), 'invalid manifest')
    const projects = await find(root, { patterns: ['.'], includeRoot: true })
    expect(projects.map(({ manifest }) => manifest.name)).toStrictEqual(['from-json'])
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})

test.each([
  ['async', findPackages],
  ['sync', findPackagesSync],
] as const)('%s discovery reports an invalid preferred manifest without falling back', async (_mode, find) => {
  const root = temporaryDirectory()
  try {
    fs.writeFileSync(path.join(root, 'package.json'), 'invalid manifest')
    fs.writeFileSync(path.join(root, 'package.json5'), "{ name: 'from-json5' }\n")
    await expect(Promise.resolve().then(async () => find(root, { patterns: ['.'] }))).rejects.toThrow()
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})
