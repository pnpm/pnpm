import { checkbox, Separator } from '@inquirer/prompts'
import { interactivePromptPageSize } from '@pnpm/cli.utils'
import type { AuditReport } from '@pnpm/deps.compliance.audit'
import { isError } from '@pnpm/error'
import { globalInfo } from '@pnpm/logger'
import type { RangeSpecStyle } from '@pnpm/types'
import chalk from 'chalk'

import { getAuditFixChoices } from './getAuditFixChoices.js'

type AuditFixCheckboxChoice = Separator | { name: string; value: string; short: string; disabled?: boolean | string }

export async function interactiveAuditFix (auditReport: AuditReport, rangeSpecStyle: RangeSpecStyle): Promise<AuditReport> {
  const choiceGroups = getAuditFixChoices(Object.values(auditReport.advisories), rangeSpecStyle)
  if (choiceGroups.length === 0) {
    return auditReport
  }

  const selectedKeys = await promptForAdvisoriesToFix(toCheckboxChoices(choiceGroups))

  const selectedKeySet = new Set(selectedKeys)
  const selectedAdvisories = Object.fromEntries(
    Object.entries(auditReport.advisories)
      .filter(([, advisory]) =>
        selectedKeySet.has(`${advisory.module_name}@${advisory.vulnerable_versions}`)
      )
  )
  return { ...auditReport, advisories: selectedAdvisories }
}

function toCheckboxChoices (choiceGroups: ReturnType<typeof getAuditFixChoices>): AuditFixCheckboxChoice[] {
  const flatChoices: AuditFixCheckboxChoice[] = []
  for (const group of choiceGroups) {
    flatChoices.push(new Separator(chalk.bold(`── ${group.message} ──`)))
    for (const choice of group.choices) {
      flatChoices.push(toCheckboxChoice(choice))
    }
  }
  return flatChoices
}

function toCheckboxChoice (choice: ReturnType<typeof getAuditFixChoices>[number]['choices'][number]): AuditFixCheckboxChoice {
  if (choice.disabled) {
    return new Separator(`  ${choice.message ?? choice.name}`)
  }
  return {
    name: choice.message,
    value: choice.value,
    // Same shape as the update prompt: `name` is the rendered table
    // row, but the post-submission line uses `short` per choice.
    // Without this, every selected row's full table dump is comma-
    // joined back to stdout.
    short: choice.value,
  }
}

async function promptForAdvisoriesToFix (choices: AuditFixCheckboxChoice[]): Promise<string[]> {
  const message = 'Choose which vulnerabilities to fix ' +
    `(Press ${chalk.cyan('<space>')} to select, ` +
    `${chalk.cyan('<a>')} to toggle all, ` +
    `${chalk.cyan('<i>')} to invert selection)\n\nEnter to start fixing. Ctrl-c to cancel.`
  try {
    return await checkbox({
      choices,
      pageSize: interactivePromptPageSize(),
      message,
      required: true,
      validate: (values) => {
        if (values.length === 0) {
          return 'You must choose at least one vulnerability.'
        }
        return true
      },
      theme: {
        icon: { checked: '●', unchecked: '○', cursor: '❯' },
        style: {
          highlight: (text: string) => text,
        },
        keybindings: ['vim'],
      },
    })
  } catch (err) {
    if (isError(err) && err.name === 'ExitPromptError') {
      globalInfo('Audit fix canceled')
      // eslint-disable-next-line n/no-process-exit -- canceling the prompt ends the command successfully without applying any fix
      process.exit(0)
    }
    throw err
  }
}
