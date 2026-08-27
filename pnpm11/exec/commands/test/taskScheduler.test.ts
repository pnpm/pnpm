import { expect, test } from '@jest/globals'
import type { ProjectRootDir } from '@pnpm/types'

import { type TaskGraph, taskKey } from '../src/taskGraph.js'
import { scheduleTasks } from '../src/taskScheduler.js'

test('an aborted task stops dispatch and the scheduler still settles', async () => {
  const graph = chainGraph()
  const ran: string[] = []
  await scheduleTasks(graph, {
    bail: false,
    runTask: async (node) => {
      ran.push(node.project)
      return node.project.endsWith('/a') ? 'aborted' : 'passed'
    },
    onTaskSkipped: () => {
      throw new Error('an abort leaves undispatched tasks queued, not skipped')
    },
  })
  expect(ran).toStrictEqual(['/workspace/a'])
})

test('a rejected runTask stops dispatch and resurfaces as the scheduler\'s failure', async () => {
  const graph = chainGraph()
  const ran: string[] = []
  await expect(scheduleTasks(graph, {
    bail: false,
    runTask: async (node) => {
      ran.push(node.project)
      throw new Error('boom')
    },
    onTaskSkipped: () => {},
  })).rejects.toThrow('boom')
  expect(ran).toStrictEqual(['/workspace/a'])
})

function chainGraph (): TaskGraph {
  const graph: TaskGraph = new Map()
  let previous: string | undefined
  for (const name of ['a', 'b', 'c']) {
    const project = `/workspace/${name}` as ProjectRootDir
    const key = taskKey(project, 'build')
    graph.set(key, {
      project,
      taskName: 'build',
      scripts: ['build'],
      requested: true,
      dependencies: previous == null ? [] : [previous],
    })
    previous = key
  }
  return graph
}
