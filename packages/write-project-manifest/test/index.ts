/// <reference path="../../../typings/index.d.ts"/>
import { promisify } from 'util'
import fs from 'fs'
import path from 'path'
import writeProjectManifest from '@pnpm/write-project-manifest'
import tempy from 'tempy'

const readFile = promisify(fs.readFile)

test('writeProjectManifest()', async () => {
  const dir = tempy.directory()

  await writeProjectManifest(path.join(dir, 'package.json'), { name: 'foo', version: '1.0.0' })
  expect(await readFile(path.join(dir, 'package.json'), 'utf8')).toBe('{\n\t"name": "foo",\n\t"version": "1.0.0"\n}\n')

  await writeProjectManifest(path.join(dir, 'package.json5'), { name: 'foo', version: '1.0.0' })
  expect(await readFile(path.join(dir, 'package.json5'), 'utf8')).toBe("{\n\tname: 'foo',\n\tversion: '1.0.0',\n}\n")

  await writeProjectManifest(path.join(dir, 'package.yaml'), { name: 'foo', version: '1.0.0' })
  expect(await readFile(path.join(dir, 'package.yaml'), 'utf8')).toBe('name: foo\nversion: 1.0.0\n')
})
