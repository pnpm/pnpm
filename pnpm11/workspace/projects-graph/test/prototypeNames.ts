/// <reference path="../../../__typings__/local.d.ts"/>
import { expect, test } from '@jest/globals'
import { createProjectsGraph } from '@pnpm/workspace.projects-graph'
import { betterPathResolve as pathResolve } from 'better-path-resolve'

const APP_PATH = pathResolve('/zkochan/src/app')
const CONSTRUCTOR_PATH = pathResolve('/zkochan/src/constructor')

test('createProjectsGraph() handles a dependency named like an Object.prototype member', () => {
  const result = createProjectsGraph([
    {
      rootDir: APP_PATH,
      manifest: {
        name: 'app',
        version: '1.0.0',
        dependencies: {
          constructor: '^1.0.0',
        },
      },
    },
  ])
  expect(result.graph[APP_PATH].dependencies).toStrictEqual([])
})

test('createProjectsGraph() links a workspace project named like an Object.prototype member', () => {
  const result = createProjectsGraph([
    {
      rootDir: APP_PATH,
      manifest: {
        name: 'app',
        version: '1.0.0',
        dependencies: {
          constructor: '^1.0.0',
        },
      },
    },
    {
      rootDir: CONSTRUCTOR_PATH,
      manifest: {
        name: 'constructor',
        version: '1.0.0',
      },
    },
  ])
  expect(result.graph[APP_PATH].dependencies).toStrictEqual([CONSTRUCTOR_PATH])
})
