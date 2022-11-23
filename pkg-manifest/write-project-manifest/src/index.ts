import { promises as fs } from 'fs'
import path from 'path'
import { ProjectManifest } from '@pnpm/types'
import JSON5 from 'json5'
import writeFileAtomic from 'write-file-atomic'
import writeYamlFile from 'write-yaml-file'

const YAML_FORMAT = {
  noCompatMode: true,
  noRefs: true,
}

export interface CommentSpecifier {
  type: string
  content: string
  lineNumber: number
  after?: string
  on: string
  whitespace: string
  before?: string
}

export async function writeProjectManifest (
  filePath: string,
  manifest: ProjectManifest,
  opts?: {
    comments?: CommentSpecifier[]
    indent?: string | number | undefined
    insertFinalNewline?: boolean
  }
): Promise<void> {
  const fileType = filePath.slice(filePath.lastIndexOf('.') + 1).toLowerCase()
  if (fileType === 'yaml') {
    return writeYamlFile(filePath, manifest, YAML_FORMAT)
  }

  await fs.mkdir(path.dirname(filePath), { recursive: true })
  const trailingNewline = opts?.insertFinalNewline === false ? '' : '\n'

  let json = (fileType === 'json5' ? JSON5 : JSON)
    .stringify(manifest, undefined, opts?.indent ?? '\t')

  if (opts?.comments) {
    // We need to reintroduce the comments. So create an index of
    // the lines of the manifest so we can try to match them up.
    // We eliminate whitespace and quotes because pnpm may have changed them.
    const jsonLines = json.split('\n')
    const index = {}
    for (let i = 0; i < jsonLines.length; ++i) {
      const key = jsonLines[i].replace(/[\s'"]/g, '')
      if (key in index) {
        index[key] = -1
      } else {
        index[key] = i
      }
    }

    // A place to put comments that come _before_ the lines they are
    // anchored to:
    const jsonPrefix: Record<string, string> = {}
    for (const comment of opts.comments) {
      // First if we can find the line the comment was on, that is
      // the most reliable locator:
      let key = comment.on.replace(/[\s'"]/g, '')
      if (key && index[key] !== undefined && index[key] >= 0) {
        jsonLines[index[key]] += ' ' + comment.content
        continue
      }
      // Next, if it's not before anything, it must have been at the very end:
      if (comment.before === undefined) {
        jsonLines[jsonLines.length - 1] += comment.whitespace + comment.content
        continue
      }
      // Next, try to put it before something; note the comment extractor
      // used the convention that position 0 is before the first line:
      let location = (comment.lineNumber === 0) ? 0 : -1
      if (location < 0) {
        key = comment.before.replace(/[\s'"]/g, '')
        if (key && index[key] !== undefined) {
          location = index[key]
        }
      }
      if (location >= 0) {
        if (jsonPrefix[location]) {
          jsonPrefix[location] += ' ' + comment.content
        } else {
          const inlineWhitespace = comment.whitespace.startsWith('\n')
            ? comment.whitespace.slice(1)
            : comment.whitespace
          jsonPrefix[location] = inlineWhitespace + comment.content
        }
        continue
      }
      // The last definite indicator we can use is that it is after something:
      if (comment.after) {
        key = comment.after.replace(/[\s'"]/g, '')
        if (key && index[key] !== undefined && index[key] >= 0) {
          jsonLines[index[key]] += comment.whitespace + comment.content
          continue
        }
      }
      // Finally, try to get it in the right general location by using the
      // line number, but warn the user the comment may have been relocated:
      location = comment.lineNumber - 1 // 0 was handled above
      let separator = ' '
      if (location >= jsonLines.length) {
        location = jsonLines.length - 1
        separator = '\n'
      }
      jsonLines[location] += separator + comment.content +
        ' /* [comment possibly relocated by pnpm] */'
    }
    // Insert the accumulated prefixes:
    for (let i = 0; i < jsonLines.length; ++i) {
      if (jsonPrefix[i]) {
        jsonLines[i] = jsonPrefix[i] + '\n' + jsonLines[i]
      }
    }
    // And reassemble the manifest:
    json = jsonLines.join('\n')
  }

  return writeFileAtomic(filePath, `${json}${trailingNewline}`)
}
