// The TypeScript counterparts of perfectionist's comment rules.

function createCommentRule ({ description, message, schema = [], findings }) {
  return {
    meta: {
      type: 'suggestion',
      docs: { description },
      schema,
      messages: { finding: message },
    },
    create (context) {
      return {
        Program () {
          for (const comment of context.sourceCode.getAllComments()) {
            for (const { index, length, data } of findings(comment.value, context.options[0] ?? {})) {
              context.report({ loc: commentLoc(context.sourceCode, comment, index, length), messageId: 'finding', data })
            }
          }
        },
      }
    },
  }
}

function commentLoc (sourceCode, comment, index, length) {
  // `comment.value` starts after the `//` or `/*` that opens the comment.
  const start = comment.range[0] + 2 + index
  return { start: sourceCode.getLocFromIndex(start), end: sourceCode.getLocFromIndex(start + length) }
}

function * matchAll (text, regex, toData = (match) => ({ text: match[0] })) {
  for (const match of text.matchAll(regex)) {
    const data = toData(match)
    if (data != null) yield { index: match.index, length: match[0].length, data }
  }
}

// `#123` with nothing before it that names a repository, such as `owner/repo#123`,
// and outside backticks.
const BARE_ISSUE_REFERENCE = /(?<![\w/.&-])#\d+\b/g

export const bareIssueReference = createCommentRule({
  description: 'Disallow ambiguous bare `#NNN` references in comments',
  message: '`{{text}}` is an ambiguous issue reference. Write `owner/repo{{text}}` or link the full URL.',
  findings: (text) => matchAll(withoutCodeSpans(text), BARE_ISSUE_REFERENCE),
})

export const unicodeEllipsisInComments = createCommentRule({
  description: 'Disallow the Unicode ellipsis character in comments',
  message: 'Write three periods (`...`) instead of the Unicode ellipsis character.',
  findings: (text) => matchAll(text, /…/g),
})

const FORGE_REF = /https?:\/\/(?:www\.)?(?:github\.com|gitlab\.com|codeberg\.org|bitbucket\.org)\/[^/\s]+\/[^/\s]+\/(?:-\/)?(?:blob|tree|raw|blame|src)\/([^/\s#?)>\]]+)/g
const COMMIT_SHA = /^[0-9a-f]{4,40}$/i
const VERSION_REF = /^(?:[\w.-]+@)?v?\d+(?:\.\d+)*(?:-[\w.]+)?$/

export const unpinnedRepoRef = createCommentRule({
  description: 'Disallow links to a file on a branch of a hosted repository',
  message: 'This link points at `{{ref}}`, a branch that moves. Link a commit SHA or a release tag instead.',
  findings: (text) => matchAll(text, FORGE_REF, (match) => {
    const ref = match[1]
    return COMMIT_SHA.test(ref) || VERSION_REF.test(ref) ? null : { ref }
  }),
})

// Replaces each code span with spaces, so the indices of the other text stay put.
function withoutCodeSpans (text) {
  return text.replace(/`[^`\n]*`/g, (span) => ' '.repeat(span.length))
}
