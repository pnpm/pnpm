import fs from 'node:fs'
import { gunzipSync } from 'node:zlib'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { publish } from '@pnpm/releasing.commands'
import { writePendingChangelog } from '@pnpm/releasing.versioning'
import { getRegistryMockToken, REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import tar from 'tar-stream'

import { DEFAULT_OPTS } from './utils/index.js'

const REGISTRY_URL = `http://localhost:${REGISTRY_MOCK_PORT}`
const CONFIG_BY_URI = {
  [`//localhost:${REGISTRY_MOCK_PORT}/`]: {
    '@': { authToken: getRegistryMockToken() },
  },
}
// The mock's `local` hosted registry only accepts packages in its declared
// namespaces (`@pnpm.e2e/*` is one); other names route to the read-only
// upstream proxy, which refuses publishes.
const PKG_NAME = '@pnpm.e2e/registry-changelog-e2e'

/**
 * Simulates a `registry`-storage release of `version`: bump the manifest and
 * park the composed changelog section, exactly as `pnpm version -r` does.
 * `pnpm publish` then composes and packs the CHANGELOG.md from it.
 */
async function releaseAndPublish (version: string, section: string): Promise<void> {
  const manifest = JSON.parse(fs.readFileSync('package.json', 'utf8'))
  manifest.version = version
  fs.writeFileSync('package.json', JSON.stringify(manifest, null, 2))
  await writePendingChangelog(process.cwd(), PKG_NAME, version, section)

  await publish.handler({
    ...DEFAULT_OPTS,
    argv: { original: ['publish'] },
    configByUri: CONFIG_BY_URI,
    gitChecks: false,
    dir: process.cwd(),
    workspaceDir: process.cwd(),
  }, [])
}

async function fetchPublishedChangelog (version: string): Promise<string | undefined> {
  const packumentResponse = await fetch(`${REGISTRY_URL}/${PKG_NAME.replace('/', '%2F')}`)
  const packument = await packumentResponse.json() as { versions: Record<string, { dist: { tarball: string } }> }
  const tarballUrl = packument.versions[version]?.dist.tarball
  if (tarballUrl == null) return undefined
  const tarballResponse = await fetch(tarballUrl)
  const tarballData = Buffer.from(await tarballResponse.arrayBuffer())

  const extract = tar.extract()
  let changelog: string | undefined
  await new Promise<void>((resolve, reject) => {
    extract.on('entry', (header, stream, next) => {
      if (header.name !== 'package/CHANGELOG.md') {
        stream.resume()
        stream.on('end', next)
        return
      }
      const chunks: Buffer[] = []
      stream.on('data', (chunk: Buffer) => chunks.push(Buffer.from(chunk)))
      stream.on('end', () => {
        changelog = Buffer.concat(chunks).toString('utf8')
        next()
      })
    })
    extract.on('error', reject)
    extract.on('finish', resolve)
    extract.end(gunzipSync(tarballData))
  })
  return changelog
}

test('registry storage packs a composed CHANGELOG.md and prepends onto the previously published version', async () => {
  prepare({ name: PKG_NAME, version: '1.0.0' })

  await releaseAndPublish('1.1.0', '## 1.1.0\n\n### Minor Changes\n\n- First feature.\n')

  const firstChangelog = await fetchPublishedChangelog('1.1.0')
  expect(firstChangelog).toContain(`# ${PKG_NAME}`)
  expect(firstChangelog).toContain('## 1.1.0')
  expect(firstChangelog).toContain('- First feature.')

  await releaseAndPublish('1.2.0', '## 1.2.0\n\n### Minor Changes\n\n- Second feature.\n')

  const secondChangelog = await fetchPublishedChangelog('1.2.0')
  // The new section sits above the history fetched from the 1.1.0 tarball.
  expect(secondChangelog!.indexOf('## 1.2.0')).toBeLessThan(secondChangelog!.indexOf('## 1.1.0'))
  expect(secondChangelog).toContain('- Second feature.')
  expect(secondChangelog).toContain('- First feature.')
})
