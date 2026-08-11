import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { temporaryDirectory } from 'tempy'

jest.unstable_mockModule('@inquirer/prompts', () => {
  class Separator {
    separator: string
    readonly type = 'separator' as const
    constructor (separator: string) {
      this.separator = separator
    }
  }
  return {
    Separator,
    checkbox: jest.fn(),
    confirm: jest.fn(),
    input: jest.fn(),
    password: jest.fn(),
    select: jest.fn(),
  }
})
const { checkbox, input } = await import('@inquirer/prompts')
const { change } = await import('@pnpm/releasing.commands')

const mockCheckbox = jest.mocked(checkbox)
const mockInput = jest.mocked(input)

test('change: every picker pages to the terminal height', async () => {
  const workspaceDir = temporaryDirectory()
  fs.writeFileSync(path.join(workspaceDir, 'pnpm-workspace.yaml'), 'packages:\n  - packages/*\n')
  const allProjects = ['cli', 'lib'].map((name) => {
    const rootDir = path.join(workspaceDir, 'packages', name)
    fs.mkdirSync(rootDir, { recursive: true })
    const manifest = { name, version: '1.0.0' }
    fs.writeFileSync(path.join(rootDir, 'package.json'), JSON.stringify(manifest, null, 2))
    return { rootDir, manifest }
  })

  mockCheckbox.mockResolvedValueOnce(['lib']).mockResolvedValue([])
  mockInput.mockResolvedValue('Added a feature.')

  const rows = Object.getOwnPropertyDescriptor(process.stdout, 'rows')
  Object.defineProperty(process.stdout, 'rows', { value: 24, configurable: true })
  try {
    const output = await change.handler({ dir: workspaceDir, workspaceDir, allProjects } as any, []) // eslint-disable-line @typescript-eslint/no-explicit-any
    expect(output).toMatch(/Recorded change intent \.changeset\/.+\.md/)
  } finally {
    if (rows == null) {
      delete (process.stdout as Partial<Pick<NodeJS.WriteStream, 'rows'>>).rows
    } else {
      Object.defineProperty(process.stdout, 'rows', rows)
    }
  }

  expect(mockCheckbox.mock.calls).toHaveLength(3)
  for (const [options] of mockCheckbox.mock.calls) {
    expect(options).toEqual(expect.objectContaining({ pageSize: 18 }))
  }
})
