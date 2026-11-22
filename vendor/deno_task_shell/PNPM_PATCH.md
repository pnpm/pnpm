# Runtime parameter expansions

Source: crates.io `deno_task_shell` 0.33.3, https://github.com/denoland/deno_task_shell.
Upstream revision: `8c0d728c22512f799e9ec6974826d915507b4795`.
The original MIT license and source files are retained.

The parser AST adds `WordPart::ParameterExpansion`, `WordPart::FieldBoundary`,
and `Word::from_parts`. The evaluator chooses a parameter expansion's branch
from the current shell state, then evaluates only the selected word parts.
Replacement parse errors are reported only when that branch is selected.
Field boundaries preserve unquoted replacement words that span arguments.
The parser recognizes tilde expansion only in unquoted words.
Quoted interpolated values retain their whitespace.

pnpm's shell adapter supplies these AST nodes after its bounded lexical rewrite.
Environment values never become script source. Assignments, exported variables,
and subshell scope are resolved by the shell evaluator.

Remove this patch when upstream supports equivalent runtime parameter expansions.
