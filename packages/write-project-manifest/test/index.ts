/// <reference path="../../../typings/index.d.ts"/>
import { promisify } from 'util'
import writeProjectManifest from '@pnpm/write-project-manifest'
import fs = require('fs')
import path = require('path')
import test = require('tape')
import tempy = require('tempy')

const readFile = promisify(fs.readFile)

test('writeProjectManifest()', async (t) => {
  const dir = tempy.directory()

  await writeProjectManifest(path.join(dir, 'package.json'), { name: 'foo', version: '1.0.0' })
  t.equal(await readFile(path.join(dir, 'package.json'), 'utf8'), '{\n\t"name": "foo",\n\t"version": "1.0.0"\n}\n')

  await writeProjectManifest(path.join(dir, 'package.json5'), { name: 'foo', version: '1.0.0' })
  t.equal(await readFile(path.join(dir, 'package.json5'), 'utf8'), "{\n\tname: 'foo',\n\tversion: '1.0.0',\n}\n")

  await writeProjectManifest(path.join(dir, 'package.yaml'), { name: 'foo', version: '1.0.0' })
  t.equal(await readFile(path.join(dir, 'package.yaml'), 'utf8'), 'name: foo\nversion: 1.0.0\n')

  t.end()
})
