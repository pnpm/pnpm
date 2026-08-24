import fs from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'

import { complete } from '../../src/completion/complete.js'

test('complete an option value', async () => {
  const completions = await complete(
    {
      cliOptionsTypesByCommandName: {
        install: () => ({
          'resolution-strategy': ['fast', 'fewer-dependencies'],
        }),
      },
      completionByCommandName: {},
      initialCompletion: () => [],
      shorthandsByCommandName: {},
      universalOptionsTypes: {},
      universalShorthands: {},
    },
    {
      cmd: 'install',
      currentTypedWordType: null,
      lastOption: '--resolution-strategy',
      options: {},
      params: [],
    }
  )
  expect(completions).toStrictEqual([
    { name: 'fast' },
    { name: 'fewer-dependencies' },
  ])
})

test('complete workspace packages from the root when the workspace manifest has no packages field', async () => {
  const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-completion-'))
  const initialCwd = process.cwd()
  try {
    await fs.mkdir(path.join(workspaceDir, 'nested'), { recursive: true })
    await Promise.all([
      fs.writeFile(path.join(workspaceDir, 'package.json'), JSON.stringify({ name: 'root' })),
      fs.writeFile(path.join(workspaceDir, 'pnpm-workspace.yaml'), 'minimumReleaseAge: 0\n'),
      fs.writeFile(path.join(workspaceDir, 'nested/package.json'), JSON.stringify({ name: 'nested' })),
    ])
    process.chdir(workspaceDir)

    const completions = await complete(
      {
        cliOptionsTypesByCommandName: {},
        completionByCommandName: {},
        initialCompletion: () => [],
        shorthandsByCommandName: {},
        universalOptionsTypes: {},
        universalShorthands: {},
      },
      {
        cmd: 'run',
        currentTypedWordType: 'value',
        lastOption: '--filter',
        options: {},
        params: [],
      }
    )

    expect(completions).toStrictEqual([{ name: 'root' }])
  } finally {
    process.chdir(initialCwd)
    await fs.rm(workspaceDir, { recursive: true, force: true })
  }
})

test('complete nested packages when there is no workspace manifest', async () => {
  const workspaceDir = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-completion-'))
  const initialCwd = process.cwd()
  try {
    await fs.mkdir(path.join(workspaceDir, 'nested'), { recursive: true })
    await Promise.all([
      fs.writeFile(path.join(workspaceDir, 'package.json'), JSON.stringify({ name: 'root' })),
      fs.writeFile(path.join(workspaceDir, 'nested/package.json'), JSON.stringify({ name: 'nested' })),
    ])
    process.chdir(workspaceDir)

    const completions = await complete(
      {
        cliOptionsTypesByCommandName: {},
        completionByCommandName: {},
        initialCompletion: () => [],
        shorthandsByCommandName: {},
        universalOptionsTypes: {},
        universalShorthands: {},
      },
      {
        cmd: 'run',
        currentTypedWordType: 'value',
        lastOption: '--filter',
        options: {},
        params: [],
      }
    )

    expect(completions).toStrictEqual([{ name: 'root' }, { name: 'nested' }])
  } finally {
    process.chdir(initialCwd)
    await fs.rm(workspaceDir, { recursive: true, force: true })
  }
})

test('complete a command', async () => {
  const ctx = {
    cliOptionsTypesByCommandName: {
      run: () => ({
        'if-present': Boolean,
      }),
    },
    completionByCommandName: {
      run: async () => [{ name: 'test' }],
    },
    initialCompletion: () => [],
    shorthandsByCommandName: {},
    universalOptionsTypes: {
      filter: String,
    },
    universalShorthands: {},
  }
  expect(
    await complete(ctx,
      {
        cmd: 'run',
        currentTypedWordType: 'value',
        lastOption: null,
        options: {},
        params: [],
      }
    )
  ).toStrictEqual(
    [{ name: 'test' }]
  )
  expect(
    await complete(ctx,
      {
        cmd: 'run',
        currentTypedWordType: null,
        lastOption: null,
        options: {},
        params: [],
      }
    )
  ).toStrictEqual(
    [
      { name: 'test' },
      { name: '--filter' },
      { name: '--if-present' },
      { name: '--no-if-present' },
    ]
  )
  expect(
    await complete(ctx,
      {
        cmd: 'run',
        currentTypedWordType: 'option',
        lastOption: null,
        options: {},
        params: [],
      }
    )
  ).toStrictEqual(
    [
      { name: '--filter' },
      { name: '--if-present' },
      { name: '--no-if-present' },
    ]
  )
})

test('if command completion fails, return empty array', async () => {
  expect(
    await complete(
      {
        cliOptionsTypesByCommandName: {},
        completionByCommandName: {
          run: async () => {
            throw new Error('error')
          },
        },
        initialCompletion: () => [],
        shorthandsByCommandName: {},
        universalOptionsTypes: {
          filter: String,
        },
        universalShorthands: {},
      },
      {
        cmd: 'run',
        currentTypedWordType: 'value',
        lastOption: null,
        options: {},
        params: [],
      }
    )
  ).toStrictEqual(
    []
  )
})

test('initial completion', async () => {
  const ctx = {
    cliOptionsTypesByCommandName: {},
    completionByCommandName: {},
    initialCompletion: () => [
      { name: 'add' },
      { name: 'install' },
    ],
    shorthandsByCommandName: {},
    universalOptionsTypes: {
      filter: String,
    },
    universalShorthands: {},
  }
  expect(
    await complete(ctx,
      {
        cmd: null,
        currentTypedWordType: null,
        lastOption: null,
        options: {},
        params: [],
      }
    )
  ).toStrictEqual([
    { name: 'add' },
    { name: 'install' },
    { name: '--filter' },
    { name: '--version' },
  ])
  expect(
    await complete(ctx,
      {
        cmd: 'ad',
        currentTypedWordType: 'value',
        lastOption: null,
        options: {},
        params: [],
      }
    )
  ).toStrictEqual([
    { name: 'add' },
    { name: 'install' },
  ])
  expect(
    await complete(ctx,
      {
        cmd: null,
        currentTypedWordType: 'option',
        lastOption: null,
        options: {},
        params: [],
      }
    )
  ).toStrictEqual([
    { name: '--filter' },
    { name: '--version' },
  ])
})

test('suggest no completions for after --version', async () => {
  expect(
    await complete(
      {
        cliOptionsTypesByCommandName: {},
        completionByCommandName: {},
        initialCompletion: () => [
          { name: 'add' },
          { name: 'install' },
        ],
        shorthandsByCommandName: {},
        universalOptionsTypes: {},
        universalShorthands: {},
      },
      {
        cmd: null,
        currentTypedWordType: null,
        lastOption: null,
        options: { version: true },
        params: [],
      }
    )
  ).toStrictEqual([])
})
