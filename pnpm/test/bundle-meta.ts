import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

// From https://esbuild.github.io/api/#metafile
interface Metafile {
  inputs: {
    [path: string]: {
      bytes: number
      imports: Array<{
        path: string
        kind: string
        external?: boolean
        original?: string
        with?: Record<string, string>
      }>
      format?: string
      with?: Record<string, string>
    }
  }
  outputs: {
    [path: string]: {
      bytes: number
      inputs: {
        [path: string]: {
          bytesInOutput: number
        }
      }
      imports: Array<{
        path: string
        kind: string
        external?: boolean
      }>
      exports: string[]
      entryPoint?: string
      cssBundle?: string
    }
  }
}

let mainMeta: Metafile
let workerMeta: Metafile

beforeAll(async () => {
  let mainMetaBuf
  let workerMetaBuf

  try {
    [mainMetaBuf, workerMetaBuf] = await Promise.all([
      fs.promises.readFile(path.join(import.meta.dirname, '../stats/meta.json')),
      fs.promises.readFile(path.join(import.meta.dirname, '../stats/meta-worker.json')),
    ])
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      throw new Error('This test request esbuild metafiles to be created. Please build pnpm before running this test.')
    }

    throw err
  }

  mainMeta = JSON.parse(mainMetaBuf.toString())
  workerMeta = JSON.parse(workerMetaBuf.toString())
})

// ## Purpose
//
// pnpm uses the 'yaml' library (https://npmjs.org/package/yaml). Prevent
// accidental usages of js-yaml, which would inflate pnpm's final distribution
// size.
//
// ## Failures
//
// If this test fails on your branch, you can use esbuild's analyze flag to see
// the import path of the files causing js-yaml to be bundled.
//
// https://esbuild.github.io/api/#analyze
test('bundle does not have js-yaml', () => {
  function isJsYamlFile (file: string) {
    return file.includes('/node_modules/js-yaml/') || file.includes('/node_modules/@zkochan/js-yaml/')
  }

  const jsYamlInMainBundle = Object.keys(mainMeta.inputs).filter(isJsYamlFile)
  expect(jsYamlInMainBundle).toEqual([])

  const jsYamlInWorkerBundle = Object.keys(workerMeta.inputs).filter(isJsYamlFile)
  expect(jsYamlInWorkerBundle).toEqual([])
})
