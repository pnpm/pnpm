use super::{
    Parameter,
    PreparedScript,
};
use deno_task_shell::parser::{
    self,
    Command,
    CommandInner,
    IoFile,
    PipelineInner,
    Sequence,
    SequentialList,
    Word,
    WordPart,
};

pub(super) fn resolve(list: &mut SequentialList, prepared: &PreparedScript) {
    let mut parameters = Vec::with_capacity(prepared.parameters.len());
    for parameter in &prepared.parameters {
        let word = parse_replacement(parameter)
            .map(|mut word| {
                resolve_parts(&mut word, &prepared.prefix, &parameters);
                word
            });
        parameters.push(WordPart::ParameterExpansion {
            name: parameter.name.clone(),
            empty_is_unset: parameter.empty_is_unset,
            alternate: parameter.alternate,
            word,
        });
    }
    resolve_list(list, &prepared.prefix, &parameters);
}

fn parse_replacement(parameter: &Parameter) -> Result<Vec<WordPart>, String> {
    let word = &parameter.word;
    let source = if parameter.quoted {
        format!(r#"__pnpm_word "{word}""#)
    } else {
        format!("__pnpm_word {word}")
    };
    let mut parsed = parser::parse(&source).map_err(|error| error.to_string())?;
    if parsed.items.len() != 1 {
        return Err("invalid parameter expansion word".to_string());
    }
    let Some(Sequence::Pipeline(pipeline)) = parsed.items.pop().map(|item| item.sequence) else {
        return Err("invalid parameter expansion word".to_string());
    };
    let PipelineInner::Command(Command {
        inner: CommandInner::Simple(command),
        ..
    }) = pipeline.inner
    else {
        return Err("invalid parameter expansion command".to_string());
    };
    Ok(replacement_parts(command.args.into_iter().skip(1), parameter))
}

fn replacement_parts(words: impl Iterator<Item = Word>, parameter: &Parameter) -> Vec<WordPart> {
    let word = &parameter.word;
    let mut parts = Vec::new();
    if !parameter.quoted && word.starts_with(char::is_whitespace) {
        parts.push(WordPart::FieldBoundary);
    }
    for (index, word) in words.enumerate() {
        if index != 0 {
            parts.push(WordPart::FieldBoundary);
        }
        let mut word = word.into_parts();
        remove_empty_text(&mut word);
        parts.extend(word);
    }
    if !parameter.quoted && word.ends_with(char::is_whitespace) {
        parts.push(WordPart::FieldBoundary);
    }
    parts
}

fn remove_empty_text(parts: &mut Vec<WordPart>) {
    parts.retain(|part| !matches!(part, WordPart::Text(text) if text.is_empty()));
    for part in parts {
        if let WordPart::Quoted(parts) = part {
            remove_empty_text(parts);
        }
    }
}

fn resolve_list(list: &mut SequentialList, prefix: &str, parameters: &[WordPart]) {
    for item in &mut list.items {
        resolve_sequence(&mut item.sequence, prefix, parameters);
    }
}

fn resolve_sequence(sequence: &mut Sequence, prefix: &str, parameters: &[WordPart]) {
    match sequence {
        Sequence::ShellVar(variable) => resolve_word(&mut variable.value, prefix, parameters),
        Sequence::Pipeline(pipeline) => resolve_pipeline(&mut pipeline.inner, prefix, parameters),
        Sequence::BooleanList(list) => {
            resolve_sequence(&mut list.current, prefix, parameters);
            resolve_sequence(&mut list.next, prefix, parameters);
        }
    }
}

fn resolve_pipeline(pipeline: &mut PipelineInner, prefix: &str, parameters: &[WordPart]) {
    match pipeline {
        PipelineInner::Command(command) => resolve_command(command, prefix, parameters),
        PipelineInner::PipeSequence(sequence) => {
            resolve_command(&mut sequence.current, prefix, parameters);
            resolve_pipeline(&mut sequence.next, prefix, parameters);
        }
    }
}

fn resolve_command(command: &mut Command, prefix: &str, parameters: &[WordPart]) {
    if let Some(redirect) = &mut command.redirect
        && let IoFile::Word(word) = &mut redirect.io_file
    {
        resolve_word(word, prefix, parameters);
    }
    match &mut command.inner {
        CommandInner::Simple(command) => {
            for variable in &mut command.env_vars {
                resolve_word(&mut variable.value, prefix, parameters);
            }
            for word in &mut command.args {
                resolve_word(word, prefix, parameters);
            }
        }
        CommandInner::Subshell(list) => resolve_list(list, prefix, parameters),
    }
}

fn resolve_word(word: &mut Word, prefix: &str, parameters: &[WordPart]) {
    let mut parts = std::mem::replace(word, Word::new_word("")).into_parts();
    resolve_parts(&mut parts, prefix, parameters);
    *word = Word::from_parts(parts);
}

fn resolve_parts(parts: &mut [WordPart], prefix: &str, parameters: &[WordPart]) {
    for part in parts {
        match part {
            WordPart::Variable(name) => {
                if let Some(parameter) = name
                    .strip_prefix(prefix)
                    .map(str::parse::<usize>)
                    .and_then(Result::ok)
                    .and_then(|index| parameters.get(index))
                {
                    *part = parameter.clone();
                }
            }
            WordPart::Quoted(parts) => resolve_parts(parts, prefix, parameters),
            WordPart::Command(list) => resolve_list(list, prefix, parameters),
            WordPart::Brace(words) => {
                for word in words {
                    resolve_word(word, prefix, parameters);
                }
            }
            WordPart::Text(_)
            | WordPart::Tilde
            | WordPart::FieldBoundary
            | WordPart::ParameterExpansion { .. } => {}
        }
    }
}
