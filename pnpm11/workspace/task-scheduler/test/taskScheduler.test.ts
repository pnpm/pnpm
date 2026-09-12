import { expect, test } from '@jest/globals'
import type { ProjectRootDir } from '@pnpm/types'
import { scheduleGraph, scheduleTasks, type TaskGraph, taskKey } from '@pnpm/workspace.task-scheduler'

test('a dependent starts without waiting for an unrelated slow branch', async () => {
  let releaseSlow!: () => void
  const slow = new Promise<void>((resolve) => {
    releaseSlow = resolve
  })
  const ran: string[] = []
  await scheduleGraph(new Map([
    ['slow', []],
    ['fast', []],
    ['dependent', ['fast']],
  ]), {
    bail: true,
    concurrency: 2,
    runNode: async (node) => {
      ran.push(node)
      if (node === 'slow') await slow
      if (node === 'dependent') releaseSlow()
      return 'passed'
    },
    onNodeSkipped: () => {},
  })
  expect(ran).toStrictEqual(['slow', 'fast', 'dependent'])
})

test('continueOnFailure preserves legacy no-bail command behavior', async () => {
  const ran: string[] = []
  await scheduleGraph(new Map([
    ['dependency', []],
    ['dependent', ['dependency']],
  ]), {
    bail: false,
    concurrency: 1,
    continueOnFailure: true,
    runNode: async (node) => {
      ran.push(node)
      return node === 'dependency' ? 'failed' : 'passed'
    },
    onNodeSkipped: () => {},
  })
  expect(ran).toStrictEqual(['dependency', 'dependent'])
})

test('a bailed graph waits for work that was already dispatched', async () => {
  let releaseSlow!: () => void
  const slow = new Promise<void>((resolve) => {
    releaseSlow = resolve
  })
  let finished = false
  const ran: string[] = []
  const scheduled = scheduleGraph(new Map([
    ['slow', []],
    ['failed', []],
    ['queued', []],
  ]), {
    bail: true,
    concurrency: 2,
    runNode: async (node) => {
      ran.push(node)
      if (node === 'slow') await slow
      return node === 'failed' ? 'failed' : 'passed'
    },
    onNodeSkipped: () => {},
  }).then(() => {
    finished = true
  })
  await Promise.resolve()
  await Promise.resolve()
  expect(finished).toBe(false)
  releaseSlow()
  await scheduled
  expect(ran).not.toContain('queued')
})

test.each([NaN, 0, -1, 1.5])('invalid concurrency %p settles with one active node', async (concurrency) => {
  let active = 0
  let maxActive = 0
  await scheduleGraph(new Map([
    ['first', []],
    ['second', []],
  ]), {
    bail: true,
    concurrency,
    runNode: async () => {
      active++
      maxActive = Math.max(maxActive, active)
      await Promise.resolve()
      active--
      return 'passed'
    },
    onNodeSkipped: () => {},
  })
  expect(maxActive).toBe(1)
})

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

test('task concurrency limits instances without blocking other task names', async () => {
  let releaseFirstBuild!: () => void
  const firstBuild = new Promise<void>((resolve) => {
    releaseFirstBuild = resolve
  })
  let activeBuilds = 0
  let maxActiveBuilds = 0
  const ran: string[] = []
  const graph = independentTaskGraph([
    ['a', 'build', 1],
    ['b', 'build', 1],
    ['c', 'lint', 1],
  ])

  await scheduleTasks(graph, {
    bail: true,
    runTask: async (node) => {
      ran.push(`${node.project}#${node.taskName}`)
      if (node.taskName === 'build') {
        activeBuilds++
        maxActiveBuilds = Math.max(maxActiveBuilds, activeBuilds)
        if (node.project.endsWith('/a')) await firstBuild
        activeBuilds--
      } else {
        releaseFirstBuild()
      }
      return 'passed'
    },
    onTaskSkipped: () => {},
  })

  expect(maxActiveBuilds).toBe(1)
  expect(ran).toStrictEqual([
    '/workspace/a#build',
    '/workspace/c#lint',
    '/workspace/b#build',
  ])
})

test('a task waiting for a concurrency permit stays undispatched after bail', async () => {
  const ran: string[] = []
  await scheduleTasks(independentTaskGraph([
    ['a', 'build', 1],
    ['b', 'build', 1],
    ['c', 'build', 1],
  ]), {
    bail: true,
    runTask: async (node) => {
      ran.push(node.project)
      return 'failed'
    },
    onTaskSkipped: () => {},
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

function independentTaskGraph (tasks: Array<[string, string, number]>): TaskGraph {
  return new Map(tasks.map(([name, taskName, concurrency]) => {
    const project = `/workspace/${name}` as ProjectRootDir
    const key = taskKey(project, taskName)
    return [key, {
      project,
      taskName,
      concurrency,
      scripts: [taskName],
      requested: true,
      dependencies: [],
    }] as const
  }))
}
