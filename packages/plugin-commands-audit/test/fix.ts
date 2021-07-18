import path from 'path'
import { copyFixture } from '@pnpm/test-fixtures'
import { ProjectManifest } from '@pnpm/types'
import { audit } from '@pnpm/plugin-commands-audit'
import loadJsonFile from 'load-json-file'
import tempy from 'tempy'

test('overrides are added for vulnerable dependencies', async () => {
  const tmp = tempy.directory()
  await copyFixture('has-vulnerabilities', tmp, __dirname)

  await audit.handler({
    auditLevel: 'moderate',
    dir: tmp,
    registries: {
      default: 'https://registry.npmjs.org/',
    },
  }, ['fix'])

  const manifest = await loadJsonFile<ProjectManifest>(path.join(tmp, 'package.json'))
  expect(manifest.pnpm?.overrides?.['axios@<0.18.1']).toBe('>=0.18.1')
})
