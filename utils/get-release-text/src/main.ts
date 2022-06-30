/// <reference path="../../../typings/local.d.ts" />
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
const pnpmDir = path.join(dirname, '../../../packages/pnpm')
const changelog = fs.readFileSync(path.join(pnpmDir, 'CHANGELOG.md'), 'utf8')
const pnpm = JSON.parse(fs.readFileSync(path.join(pnpmDir, 'package.json'), 'utf8'))
const release = getChangelogEntry(changelog, pnpm.version)
fs.writeFileSync(path.join(dirname, '../../../RELEASE.md'), release.content)

function getChangelogEntry (changelog: string, version: string) {
  const ast = unified().use(remarkParse).parse(changelog)

  let highestLevel: number = BumpLevels.dep

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
    ast['children'] = (ast['children'] as any).slice( // eslint-disable-line @typescript-eslint/no-explicit-any
      headingStartInfo.index + 1,
      endIndex
    )
  }
  return {
    content: `${unified().use(remarkStringify).stringify(ast)}

## Our Gold Sponsors

<table>
  <tbody>
    <tr>
      <td align="center" valign="middle">
        <a href="https://bit.dev/?utm_source=pnpm&utm_medium=release_notes" target="_blank"><img src="https://raw.githubusercontent.com/pnpm/pnpm.github.io/main/static/img/users/bit.svg" width="80"></a>
      </td>
      <td align="center" valign="middle">
        <a href="https://nhost.io/?utm_source=pnpm&utm_medium=release_notes" target="_blank"><img src="https://raw.githubusercontent.com/pnpm/pnpm.github.io/main/static/img/users/nhost.svg" width="180"></a>
      </td>
      <td align="center" valign="middle">
        <a href="https://novu.co/?utm_source=pnpm&utm_medium=release_notes" target="_blank"><img src="https://raw.githubusercontent.com/pnpm/pnpm.github.io/main/static/img/users/novu.svg" width="180"></a>
      </td>
    </tr>
  </tbody>
</table>

## Our Silver Sponsors

<table>
  <tbody>
    <tr>
      <td align="center" valign="middle">
        <a href="https://prisma.io/?utm_source=pnpm&utm_medium=release_notes" target="_blank"><img src="https://raw.githubusercontent.com/pnpm/pnpm.github.io/main/static/img/users/prisma.svg" width="180"></a>
      </td>
      <td align="center" valign="middle">
        <a href="https://leniolabs.com/?utm_source=pnpm&utm_medium=release_notes" target="_blank"><img src="https://raw.githubusercontent.com/pnpm/pnpm.github.io/main/static/img/users/leniolabs.jpg" width="80"></a>
      </td>
      <td align="center" valign="middle">
        <a href="https://vercel.com/?utm_source=pnpm&utm_medium=release_notes" target="_blank"><img src="https://raw.githubusercontent.com/pnpm/pnpm.github.io/main/static/img/users/vercel.svg" width="180"></a>
      </td>
      <td align="center" valign="middle">
        <a href="https://www.takeshape.io/?utm_source=pnpm&utm_medium=release_notes" target="_blank"><img src="https://raw.githubusercontent.com/pnpm/pnpm.github.io/main/static/img/users/takeshape.svg" width="280"></a>
      </td>
    </tr>
  </tbody>
</table>
`,
    highestLevel,
  }
}
