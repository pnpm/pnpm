import fs from 'fs'
import path from 'path'
import { prepare, preparePackages } from '@pnpm/prepare'
import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm, execPnpmSync } from './utils/index.js'

test('ls --filter=not-exist --json should prints an empty array (#9672)', async () => {
  preparePackages([
    {
      location: 'packages/foo',
      package: {
        name: 'foo',
        version: '0.0.0',
        private: true,
      },
    },
  ])

  writeYamlFile('pnpm-workspace.yaml', {
    packages: ['packages/*'],
  })

  const { stdout } = execPnpmSync(['ls', '--filter=project-that-does-not-exist', '--json'], { expectSuccess: true })
  expect(JSON.parse(stdout.toString())).toStrictEqual([])
})

test('ls should load a finder from .pnpmfile.cjs', async () => {
  prepare()
  const pnpmfile = `
module.exports = { finders: { hasPeerA } }
function hasPeerA (context) {
  const manifest = context.readManifest()
  if (manifest?.peerDependencies?.['@pnpm.e2e/peer-a'] == null) {
    return false
  }
  return \`@pnpm.e2e/peer-a@$\{manifest.peerDependencies['@pnpm.e2e/peer-a']}\`
}
`
  fs.writeFileSync('.pnpmfile.cjs', pnpmfile, 'utf8')
  await execPnpm(['add', 'is-positive@1.0.0', '@pnpm.e2e/abc@1.0.0'])
  const result = execPnpmSync(['list', '--find-by=hasPeerA'])
  expect(result.stdout.toString()).toMatch(`dependencies:
@pnpm.e2e/abc 1.0.0
  @pnpm.e2e/peer-a@^1.0.0`)
})

test('pnpm list returns correct paths with global virtual store', async () => {
  prepare({
    dependencies: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  })
  const storeDir = path.resolve('store')
  writeYamlFile('pnpm-workspace.yaml', {
    ci: false, // enableGlobalVirtualStore is always disabled in CI
    enableGlobalVirtualStore: true,
    storeDir,
    privateHoistPattern: '*',
  })
  await execPnpm(['install'])

  const { stdout } = execPnpmSync(['list', '--json', '--depth=Infinity'])
  const listResult = JSON.parse(stdout.toString())

  expect(listResult).toHaveLength(1)
  expect(listResult[0].dependencies).toBeDefined()

  // Verify top-level dependency path exists
  const pkgPath = listResult[0].dependencies['@pnpm.e2e/pkg-with-1-dep'].path
  expect(fs.existsSync(pkgPath)).toBeTruthy()
  expect(fs.existsSync(path.join(pkgPath, 'package.json'))).toBeTruthy()

  // Verify subdependency path exists (this tests the fix for zkochan's comment)
  const subDeps = listResult[0].dependencies['@pnpm.e2e/pkg-with-1-dep'].dependencies
  expect(subDeps).toBeDefined()
  const subDepPath = subDeps['@pnpm.e2e/dep-of-pkg-with-1-dep'].path
  expect(fs.existsSync(subDepPath)).toBeTruthy()
  expect(fs.existsSync(path.join(subDepPath, 'package.json'))).toBeTruthy()
})
