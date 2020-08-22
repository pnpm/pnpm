import complete from '../src/cmd/complete'
import test = require('tape')

test('complete an option value', async (t) => {
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
    },
    {
      cmd: 'install',
      currentTypedWordType: null,
      lastOption: '--resolution-strategy',
      options: {},
      params: [],
    }
  )
  t.deepEqual(completions, [
    { name: 'fast' },
    { name: 'fewer-dependencies' },
  ])
  t.end()
})

test('complete a command', async (t) => {
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
  }
  t.deepEqual(
    await complete(ctx,
      {
        cmd: 'run',
        currentTypedWordType: 'value',
        lastOption: null,
        options: {},
        params: [],
      }
    ),
    [{ name: 'test' }]
  )
  t.deepEqual(
    await complete(ctx,
      {
        cmd: 'run',
        currentTypedWordType: null,
        lastOption: null,
        options: {},
        params: [],
      }
    ),
    [
      { name: 'test' },
      { name: '--filter' },
      { name: '--if-present' },
      { name: '--no-if-present' },
    ]
  )
  t.deepEqual(
    await complete(ctx,
      {
        cmd: 'run',
        currentTypedWordType: 'option',
        lastOption: null,
        options: {},
        params: [],
      }
    ),
    [
      { name: '--filter' },
      { name: '--if-present' },
      { name: '--no-if-present' },
    ]
  )
  t.end()
})

test('if command completion fails, return empty array', async (t) => {
  t.deepEqual(
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
      },
      {
        cmd: 'run',
        currentTypedWordType: 'value',
        lastOption: null,
        options: {},
        params: [],
      }
    ),
    []
  )
  t.end()
})

test('initial completion', async (t) => {
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
  }
  t.deepEqual(
    await complete(ctx,
      {
        cmd: null,
        currentTypedWordType: null,
        lastOption: null,
        options: {},
        params: [],
      }
    ), [
      { name: 'add' },
      { name: 'install' },
      { name: '--filter' },
      { name: '--version' },
    ]
  )
  t.deepEqual(
    await complete(ctx,
      {
        cmd: 'ad',
        currentTypedWordType: 'value',
        lastOption: null,
        options: {},
        params: [],
      }
    ), [
      { name: 'add' },
      { name: 'install' },
    ]
  )
  t.deepEqual(
    await complete(ctx,
      {
        cmd: null,
        currentTypedWordType: 'option',
        lastOption: null,
        options: {},
        params: [],
      }
    ), [
      { name: '--filter' },
      { name: '--version' },
    ]
  )
  t.end()
})

test('suggest no completions for after --version', async (t) => {
  t.deepEqual(
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
      },
      {
        cmd: null,
        currentTypedWordType: null,
        lastOption: null,
        options: { version: true },
        params: [],
      }
    ), []
  )
  t.end()
})
