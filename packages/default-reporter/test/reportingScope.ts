import { Config } from '@pnpm/config'
import { toOutput$ } from '@pnpm/default-reporter'
import logger, {
  createStreamParser,
} from '@pnpm/logger'
import test = require('tape')

const scopeLogger = logger<object>('scope')

test('prints scope of non-recursive install in a workspace', (t) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
    },
    streamParser: createStreamParser(),
  })

  scopeLogger.debug({
    selected: 1,
    workspacePrefix: '/home/src',
  })

  t.plan(1)

  output$.take(1).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, 'Scope: current workspace package')
    },
  })
})

test('prints scope of recursive install in a workspace when not all packages are selected', (t) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as Config,
    },
    streamParser: createStreamParser(),
  })

  scopeLogger.debug({
    selected: 1,
    total: 10,
    workspacePrefix: '/home/src',
  })

  t.plan(1)

  output$.take(1).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, 'Scope: 1 of 10 workspace projects')
    },
  })
})

test('prints scope of recursive install in a workspace when all packages are selected', (t) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as Config,
    },
    streamParser: createStreamParser(),
  })

  scopeLogger.debug({
    selected: 10,
    total: 10,
    workspacePrefix: '/home/src',
  })

  t.plan(1)

  output$.take(1).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, 'Scope: all 10 workspace projects')
    },
  })
})

test('prints scope of recursive install not in a workspace when not all packages are selected', (t) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as Config,
    },
    streamParser: createStreamParser(),
  })

  scopeLogger.debug({
    selected: 1,
    total: 10,
  })

  t.plan(1)

  output$.take(1).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, 'Scope: 1 of 10 projects')
    },
  })
})

test('prints scope of recursive install not in a workspace when all packages are selected', (t) => {
  const output$ = toOutput$({
    context: {
      argv: ['install'],
      config: { recursive: true } as Config,
    },
    streamParser: createStreamParser(),
  })

  scopeLogger.debug({
    selected: 10,
    total: 10,
  })

  t.plan(1)

  output$.take(1).subscribe({
    complete: () => t.end(),
    error: t.end,
    next: output => {
      t.equal(output, 'Scope: all 10 projects')
    },
  })
})
