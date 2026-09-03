import { expect, test } from '@jest/globals'

import { buildGraph } from '../lib/buildGraph.js'

test('buildGraph() test 1', () => {
  const graph = buildGraph({
    '/a/1.0.0': {
      children: {
        c: '/c/1.0.0',
      },
      requiresBuild: true,
    },
    '/b/1.0.0': {
      children: {
        c: '/c/1.0.0',
      },
      requiresBuild: true,
    },
    '/c/1.0.0': {
      children: {},
      requiresBuild: true,
    },
  }, ['/a/1.0.0', '/b/1.0.0'])
  expect(graph).toStrictEqual(new Map([
    ['/c/1.0.0', []],
    ['/a/1.0.0', ['/c/1.0.0']],
    ['/b/1.0.0', ['/c/1.0.0']],
  ]))
})

test('buildGraph() test 2', () => {
  const graph = buildGraph({
    '/a/1.0.0': {
      children: {
        c: '/c/1.0.0',
      },
      requiresBuild: true,
    },
    '/b/1.0.0': {
      children: {
        c: '/c/1.0.0',
      },
    },
    '/c/1.0.0': {
      children: {},
      requiresBuild: true,
    },
  }, ['/a/1.0.0', '/b/1.0.0'])
  expect(graph).toStrictEqual(new Map([
    ['/c/1.0.0', []],
    ['/a/1.0.0', ['/c/1.0.0']],
  ]))
})

test('buildGraph() test 3', () => {
  const graph = buildGraph({
    '/a/1.0.0': {
      children: {
        c: '/c/1.0.0',
      },
      requiresBuild: true,
    },
    '/b/1.0.0': {
      children: {
        d: '/d/1.0.0',
      },
    },
    '/c/1.0.0': {
      children: {},
      requiresBuild: true,
    },
    '/d/1.0.0': {
      children: {
        c: '/c/1.0.0',
      },
      requiresBuild: true,
    },
  }, ['/a/1.0.0', '/b/1.0.0'])
  expect(graph).toStrictEqual(new Map([
    ['/c/1.0.0', []],
    ['/a/1.0.0', ['/c/1.0.0']],
    ['/d/1.0.0', ['/c/1.0.0']],
    ['/b/1.0.0', ['/d/1.0.0']],
  ]))
})
