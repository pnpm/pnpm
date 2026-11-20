// The TypeScript counterpart of `perfectionist::allow_attributes_without_reason`
// and `perfectionist::lint_attribute_trailing_comment`. Every
// `eslint-disable` directive states why the rule is wrong at that site, after
// a `--` separator.
const DIRECTIVE = /^\s*(eslint-disable(?:-next-line|-line)?)\b/

export default {
  meta: {
    type: 'suggestion',
    docs: {
      description: 'Require a reason on every eslint-disable directive',
    },
    schema: [],
    messages: {
      missingReason: '`{{directive}}` needs a reason. Add ` -- <why the rule is wrong here>` after the rule names.',
    },
  },
  create (context) {
    return {
      Program () {
        for (const comment of context.sourceCode.getAllComments()) {
          const match = DIRECTIVE.exec(comment.value)
          if (match == null) continue
          const reason = comment.value.split(/\s--\s/)[1]
          if (reason == null || reason.trim() === '') {
            context.report({ loc: comment.loc, messageId: 'missingReason', data: { directive: match[1] } })
          }
        }
      },
    }
  },
}
