import test = require('tape')
import complete from '../src/cmd/complete'

test('complete an option value', async (t) => {
  const completions = await complete(
    {
      cliOptionsTypesByCommandName: {
        install: () => ({
          'resolution-strategy': ['fast', 'fewer-dependencies'],
        }),
      },
      completionByCommandName: {},
      globalOptionTypes: {},
      initialCompletion: () => [],
    },
    {
      args: [],
      cmd: 'install',
      currentTypedWordType: null,
      lastOption: '--resolution-strategy',
      options: {},
    },
  )
  t.deepEqual(completions, ['fast', 'fewer-dependencies'])
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
    globalOptionTypes: {},
    initialCompletion: () => [],
  }
  t.deepEqual(
    await complete(ctx,
      {
        args: [],
        cmd: 'run',
        currentTypedWordType: 'value',
        lastOption: null,
        options: {},
      },
    ),
    [{ name: 'test' }],
  )
  t.deepEqual(
    await complete(ctx,
      {
        args: [],
        cmd: 'run',
        currentTypedWordType: null,
        lastOption: null,
        options: {},
      },
    ),
    [{ name: 'test' }, { name: '--if-present' }, { name: '--no-if-present' }],
  )
  t.deepEqual(
    await complete(ctx,
      {
        args: [],
        cmd: 'run',
        currentTypedWordType: 'option',
        lastOption: null,
        options: {},
      },
    ),
    [{ name: '--if-present' }, { name: '--no-if-present' }],
  )
  t.end()
})
