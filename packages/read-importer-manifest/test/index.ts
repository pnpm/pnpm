import { tryReadImporterManifest } from '@pnpm/read-importer-manifest'
import path = require('path')
import test = require('tape')

const fixtures = path.join(__dirname, 'fixtures')

test('readImporterManifest()', async (t) => {
  t.deepEqual(
    (await tryReadImporterManifest(path.join(fixtures, 'package-json'))).manifest,
    { name: 'foo', version: '1.0.0' },
  )

  t.deepEqual(
    (await tryReadImporterManifest(path.join(fixtures, 'package-json5'))).manifest,
    { name: 'foo', version: '1.0.0' },
  )

  t.deepEqual(
    (await tryReadImporterManifest(path.join(fixtures, 'package-yaml'))).manifest,
    { name: 'foo', version: '1.0.0' },
  )

  t.deepEqual(
    (await tryReadImporterManifest(fixtures)).manifest,
    null,
  )

  t.end()
})
