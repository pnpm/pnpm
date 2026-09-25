import fs from 'node:fs'
import path from 'node:path'

import { beforeEach, expect, jest, test } from '@jest/globals'
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

beforeEach(() => {
  jest.clearAllMocks()
  mockCheckbox.mockResolvedValueOnce(['lib']).mockResolvedValue([])
  mockInput.mockResolvedValue('Added a feature.')
})

test('change: every picker pages to the terminal height', async () => {
  await recordChangeWithStdoutRows(24)

  expect(mockCheckbox.mock.calls).toHaveLength(3)
  for (const [options] of mockCheckbox.mock.calls) {
    expect(options).toEqual(expect.objectContaining({ pageSize: 18 }))
  }
})

test('change: every picker falls back to a 7-row page when the terminal height is unknown', async () => {
  await recordChangeWithStdoutRows(undefined)

  expect(mockCheckbox.mock.calls).toHaveLength(3)
  for (const [options] of mockCheckbox.mock.calls) {
    expect(options).toEqual(expect.objectContaining({ pageSize: 7 }))
  }
})

async function recordChangeWithStdoutRows (stdoutRows: number | undefined): Promise<void> {
  const workspaceDir = temporaryDirectory()
  fs.writeFileSync(path.join(workspaceDir, 'pnpm-workspace.yaml'), 'packages:\n  - packages/*\n')
  const allProjects = ['cli', 'lib'].map((name) => {
    const rootDir = path.join(workspaceDir, 'packages', name)
    fs.mkdirSync(rootDir, { recursive: true })
    const manifest = { name, version: '1.0.0' }
    fs.writeFileSync(path.join(rootDir, 'package.json'), JSON.stringify(manifest, null, 2))
    return { rootDir, manifest }
  })

  const rows = Object.getOwnPropertyDescriptor(process.stdout, 'rows')
  Object.defineProperty(process.stdout, 'rows', { value: stdoutRows, configurable: true })
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
}
