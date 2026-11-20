import { bareIssueReference, unicodeEllipsisInComments, unpinnedRepoRef } from './comments.js'
import directives from './directives.js'
import excessiveCognitiveComplexity from './excessive-cognitive-complexity.js'
import excessiveNesting from './excessive-nesting.js'
import overlyComplexCondition from './overly-complex-condition.js'
import overlyLongFunction from './overly-long-function.js'
import overlyLongMethodChain from './overly-long-method-chain.js'
import { singleLetterClosureParam, singleLetterFunctionParam, singleLetterGeneric, singleLetterLetBinding } from './single-letter-names.js'
import tooManyLocalBindings from './too-many-local-bindings.js'

// TypeScript ports of the perfectionist rules that the Rust workspace enforces
// through `dylint.toml`. Each rule keeps the name of its Rust counterpart.
export default {
  rules: {
    'allow-directives-without-reason': directives,
    'bare-issue-reference': bareIssueReference,
    'excessive-cognitive-complexity': excessiveCognitiveComplexity,
    'excessive-nesting': excessiveNesting,
    'overly-complex-condition': overlyComplexCondition,
    'overly-long-function': overlyLongFunction,
    'overly-long-method-chain': overlyLongMethodChain,
    'single-letter-closure-param': singleLetterClosureParam,
    'single-letter-function-param': singleLetterFunctionParam,
    'single-letter-generic': singleLetterGeneric,
    'single-letter-let-binding': singleLetterLetBinding,
    'too-many-local-bindings': tooManyLocalBindings,
    'unicode-ellipsis-in-comments': unicodeEllipsisInComments,
    'unpinned-repo-ref': unpinnedRepoRef,
  },
}
