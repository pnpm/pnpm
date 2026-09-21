$results = (TabExpansion2 'pnpm run ' 9).CompletionMatches
$expected = (Get-Content package.json -Raw | ConvertFrom-Json).scripts.PSObject.Properties.Name
if ($results.Count -ne $expected.Count) {
  throw "Expected $($expected.Count) script completions, got $($results.Count)"
}
foreach ($result in $results) {
  if ($result.ListItemText -notin $expected) {
    throw "Unexpected completion: $($result.ListItemText)"
  }
  $tokens = $null
  $errors = $null
  $ast = [System.Management.Automation.Language.Parser]::ParseInput(
    'pnpm run ' + $result.CompletionText, [ref] $tokens, [ref] $errors)
  if ($errors.Count -ne 0 -or $ast.EndBlock.Statements.Count -ne 1) {
    throw "Completion does not form one command: $($result.CompletionText)"
  }
  $pipeline = $ast.EndBlock.Statements[0]
  if ($pipeline.PipelineElements.Count -ne 1) {
    throw "Completion adds pipeline syntax: $($result.CompletionText)"
  }
  $elements = $pipeline.PipelineElements[0].CommandElements
  if ($elements.Count -ne 3 -or
      $elements[2] -isnot [System.Management.Automation.Language.StringConstantExpressionAst] -or
      $elements[2].Value -ne $result.ListItemText) {
    throw "Completion does not preserve a literal script name: $($result.CompletionText)"
  }
}
$prefixes = @(
  @{ Line = 'pnpm run space` n'; Name = 'space name' },
  @{ Line = "pnpm run 'space n'"; Name = 'space name' },
  @{ Line = 'pnpm run dollar`$na'; Name = 'dollar$name' }
)
foreach ($prefix in $prefixes) {
  $matches = (TabExpansion2 $prefix.Line $prefix.Line.Length).CompletionMatches
  if ($matches.Count -ne 1 -or $matches[0].ListItemText -ne $prefix.Name) {
    throw "Completion does not match a literal prefix: $($prefix.Line)"
  }
}
$results.ListItemText | Sort-Object
