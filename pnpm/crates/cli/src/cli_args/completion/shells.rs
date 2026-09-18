pub(super) const BASH_COMPLETION: &str = r#"###-begin-pnpm-completion-###
_pnpm_completion() {
  local words cword completion
  if type _get_comp_words_by_ref &>/dev/null; then
    _get_comp_words_by_ref -n = -n @ -n : -w words -i cword
  else
    cword="$COMP_CWORD"
    words=("${COMP_WORDS[@]}")
  fi
  COMPREPLY=()
  while IFS= read -r completion; do
    COMPREPLY+=("$completion")
  done < <(COMP_CWORD="$cword" COMP_LINE="$COMP_LINE" COMP_POINT="$COMP_POINT" SHELL=bash pnpm completion-server -- "${words[@]}")
  if type __ltrim_colon_completions &>/dev/null; then
    __ltrim_colon_completions "${words[cword]}"
  fi
}
complete -F _pnpm_completion pnpm pn
###-end-pnpm-completion-###
"#;

pub(super) const FISH_COMPLETION: &str = r#"###-begin-pnpm-completion-###
function __pnpm_completion
  set -lx SHELL fish
  set -lx COMP_LINE (commandline -cp)
  set -lx COMP_POINT (string length -- $COMP_LINE)
  set -l tokens (commandline -opc)
  set -l current (commandline -ct)
  if test (count $tokens) -eq 0
    set -a tokens "$current"
  else if test "$tokens[-1]" != "$current"
    set -a tokens "$current"
  end
  pnpm completion-server -- $tokens
end
complete -c pnpm -f -a "(__pnpm_completion)"
complete -c pn -f -a "(__pnpm_completion)"
###-end-pnpm-completion-###
"#;

pub(super) const PWSH_COMPLETION: &str = r#"###-begin-pnpm-completion-###
Register-ArgumentCompleter -Native -CommandName pnpm,pn -ScriptBlock {
  param($wordToComplete, $commandAst, $cursorPosition)
  $env:SHELL = "pwsh"
  $env:COMP_LINE = $commandAst.ToString()
  $env:COMP_POINT = $cursorPosition
  $elements = @($commandAst.CommandElements | ForEach-Object {
    if ($_ -is [System.Management.Automation.Language.StringConstantExpressionAst]) { $_.Value }
    else { $_.Extent.Text }
  })
  $last = $commandAst.CommandElements[-1]
  if ($last -is [System.Management.Automation.Language.StringConstantExpressionAst] -and $last.Extent.Text -eq $wordToComplete) {
    $wordToComplete = $last.Value
  }
  if ($elements.Count -eq 0 -or $elements[-1] -ne $wordToComplete) {
    $elements += $wordToComplete
  }
  pnpm completion-server -- @elements | Where-Object { $_ } | ForEach-Object {
    $escaped = $_ -replace '\s|#|@|\$|;|,|''|\{|\}|\(|\)|"|`|\||<|>|&','`$&'
    [System.Management.Automation.CompletionResult]::new($escaped, $_, 'ParameterValue', $_)
  }
}
###-end-pnpm-completion-###
"#;

pub(super) const ZSH_COMPLETION: &str = r#"#compdef pnpm pn
###-begin-pnpm-completion-###
_pnpm_completion() {
  local reply
  reply=("${(@f)$(COMP_CWORD=$((CURRENT-1)) COMP_LINE="$BUFFER" COMP_POINT="$CURSOR" SHELL=zsh pnpm completion-server -- "${words[@]}")}")
  _describe 'values' reply
}
compdef _pnpm_completion pnpm pn
###-end-pnpm-completion-###
"#;
