///<reference path="../../../typings/index.d.ts"/>
import writeImporterManifest from '@pnpm/write-importer-manifest'
import fs = require('fs')
import path = require('path')
import test = require('tape')
import tempy = require('tempy')
import { promisify } from 'util'

const readFile = promisify(fs.readFile)

test('writeImporterManifest()', async (t) => {
  const dir = tempy.directory()

  await writeImporterManifest(path.join(dir, 'package.json'), { name: 'foo', version: '1.0.0' })
  t.equal(await readFile(path.join(dir, 'package.json'), 'utf8'), '{\n\t"name": "foo",\n\t"version": "1.0.0"\n}\n')

  await writeImporterManifest(path.join(dir, 'package.json5'), { name: 'foo', version: '1.0.0' })
  t.equal(await readFile(path.join(dir, 'package.json5'), 'utf8'), "{\n\tname: 'foo',\n\tversion: '1.0.0',\n}\n")

  await writeImporterManifest(path.join(dir, 'package.yaml'), { name: 'foo', version: '1.0.0' })
  t.equal(await readFile(path.join(dir, 'package.yaml'), 'utf8'), 'name: foo\nversion: 1.0.0\n')

  t.end()
})
