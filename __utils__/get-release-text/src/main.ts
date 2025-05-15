/// <reference path="../../../__typings__/local.d.ts" />
import fs from 'fs'
import path from 'path'
import { fileURLToPath } from 'url'
import unified from 'unified'
import remarkParse from 'remark-parse'
import remarkStringify from 'remark-stringify'
import mdastToString from 'mdast-util-to-string'

export const BumpLevels = {
  dep: 0,
  patch: 1,
  minor: 2,
  major: 3,
} as const

const dirname = path.dirname(fileURLToPath(import.meta.url))
const pnpmDir = path.join(dirname, '../../../pnpm')
const changelog = fs.readFileSync(path.join(pnpmDir, 'CHANGELOG.md'), 'utf8')
const pnpm = JSON.parse(fs.readFileSync(path.join(pnpmDir, 'package.json'), 'utf8'))
const release = getChangelogEntry(changelog, pnpm.version)
fs.writeFileSync(path.join(dirname, '../../../RELEASE.md'), release.content)

interface ChangelogEntry {
  content: string
  highestLevel: number
}

function getChangelogEntry (changelog: string, version: string): ChangelogEntry {
  const ast = unified().use(remarkParse).parse(changelog)

  let highestLevel: number = BumpLevels.dep

  // @ts-expect-error
  const nodes = ast['children'] as any[] // eslint-disable-line @typescript-eslint/no-explicit-any
  let headingStartInfo:
  | {
    index: number
    depth: number
  }
  | undefined
  let endIndex: number | undefined

  for (let i = 0; i < nodes.length; i++) {
    const node = nodes[i]
    if (node.type === 'heading') {
      const stringified: string = mdastToString(node)
      const match = stringified.toLowerCase().match(/(major|minor|patch)/)
      if (match !== null) {
        const level = BumpLevels[match[0] as 'major' | 'minor' | 'patch']
        highestLevel = Math.max(level, highestLevel)
      }
      if (headingStartInfo === undefined && stringified === version) {
        headingStartInfo = {
          index: i,
          depth: node.depth,
        }
        continue
      }
      if (
        endIndex === undefined &&
        headingStartInfo !== undefined &&
        headingStartInfo.depth === node.depth
      ) {
        endIndex = i
        break
      }
    }
  }
  if (headingStartInfo != null) {
    // @ts-expect-error
    ast['children'] = (ast['children'] as any).slice( // eslint-disable-line @typescript-eslint/no-explicit-any
      headingStartInfo.index + 1,
      endIndex
    )
  }
  return {
    content: `${unified().use(remarkStringify).stringify(ast)}

## Platinum Sponsors

<table>
  <tbody>
    <tr>
      <td align="center" valign="middle">
        <a href="https://bit.dev/?utm_source=pnpm&utm_medium=release_notes" target="_blank"><img src="https://pnpm.io/img/users/bit.svg" width="80" alt="Bit"></a>
      </td>
      <td align="center" valign="middle">
        <a href="https://sanity.io/?utm_source=pnpm&utm_medium=release_notes" target="_blank"><img src="https://pnpm.io/img/users/sanity.svg" width="180" alt="Bit"></a>
      </td>
      <td align="center" valign="middle">
        <a href="https://syntax.fm/?utm_source=pnpm&utm_medium=release_notes" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/syntaxfm.svg" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/syntaxfm_light.svg" />
            <img src="https://pnpm.io/img/users/syntaxfm.svg" width="90" alt="Syntax" />
          </picture>
        </a>
      </td>
    </tr>
  </tbody>
</table>

## Gold Sponsors

<table>
  <tbody>
    <tr>
      <td align="center" valign="middle">
        <a href="https://discord.com/?utm_source=pnpm&utm_medium=release_notes" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/discord.svg" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/discord_light.svg" />
            <img src="https://pnpm.io/img/users/discord.svg" width="220" alt="Discord" />
          </picture>
        </a>
      </td>
      <td align="center" valign="middle">
        <a href="https://uscreen.de/?utm_source=pnpm&utm_medium=release_notes" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/uscreen.svg" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/uscreen_light.svg" />
            <img src="https://pnpm.io/img/users/uscreen.svg" width="180" alt="u|screen" />
          </picture>
        </a>
      </td>
    </tr>
    <tr>
      <td align="center" valign="middle">
        <a href="https://www.jetbrains.com/?utm_source=pnpm&utm_medium=release_notes" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/jetbrains.svg" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/jetbrains.svg" />
            <img src="https://pnpm.io/img/users/jetbrains.svg" width="180" alt="JetBrains" />
          </picture>
        </a>
      </td>
      <td align="center" valign="middle">
        <a href="https://nx.dev/?utm_source=pnpm&utm_medium=release_notes" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/nx.svg?0" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/nx_light.svg?0" />
            <img src="https://pnpm.io/img/users/nx.svg" width="70" alt="Nx" />
          </picture>
        </a>
      </td>
    </tr>
    <tr>
      <td align="center" valign="middle">
        <a href="https://coderabbit.ai/?utm_source=pnpm&utm_medium=release_notes" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/coderabbit.svg" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/coderabbit_light.svg" />
            <img src="https://pnpm.io/img/users/coderabbit.svg" width="220" alt="CodeRabbit" />
          </picture>
        </a>
      </td>
      <td align="center" valign="middle">
        <a href="https://route4me.com/?utm_source=pnpm&utm_medium=release_notes" target="_blank">
          <img src="https://pnpm.io/img/users/route4me.svg" width="220" alt="Route4Me" />
        </a>
      </td>
    </tr>
    <tr>
      <td align="center" valign="middle">
        <a href="https://workleap.com/?utm_source=pnpm&utm_medium=release_notes" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/workleap.svg" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/workleap_light.svg" />
            <img src="https://pnpm.io/img/users/workleap.svg" width="190" alt="Workleap" />
          </picture>
        </a>
      </td>
      <td align="center" valign="middle">
        <a href="https://stackblitz.com/?utm_source=pnpm&utm_medium=release_notes" target="_blank">
          <picture>
            <source media="(prefers-color-scheme: light)" srcset="https://pnpm.io/img/users/stackblitz.svg" />
            <source media="(prefers-color-scheme: dark)" srcset="https://pnpm.io/img/users/stackblitz_light.svg" />
            <img src="https://pnpm.io/img/users/stackblitz.svg" width="190" alt="Stackblitz" />
          </picture>
        </a>
      </td>
    </tr>
  </tbody>
</table>
`,
    highestLevel,
  }
}
