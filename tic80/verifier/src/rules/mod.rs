//! TIC-80 source-language static rules.
//!
//! Callback discovery is bounded lexical evidence, not a complete Lua parser.
//! It recognizes conventional top-level global declarations and assignments
//! after removing Lua comments and string literals. [`has_compiled_callback`]
//! gates that bounded evidence on an independent `mlua` syntax compilation;
//! neither pass executes cartridge code.

use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    Symbol(u8),
    Newline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockEnd {
    End,
    Until,
    AwaitDo,
}

pub fn compile_without_execution(source: &str) -> Result<(), String> {
    let lua = mlua::Lua::new();
    lua.load(source)
        .set_name("cartridge")
        .into_function()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub fn has_compiled_callback(source: &str, name: &str) -> bool {
    compile_without_execution(source).is_ok() && contains_callback(source, name)
}

pub fn contains_callback(source: &str, name: &str) -> bool {
    let tokens = tokenize(source.as_bytes());
    let mut blocks = Vec::new();
    let mut elseif_pending = false;
    let mut table_depth = 0usize;
    let mut top_level_locals = BTreeSet::new();

    for (index, token) in tokens.iter().enumerate() {
        let at_top_level = blocks.is_empty() && table_depth == 0;
        if at_top_level && is_word(token, "local") {
            top_level_locals.extend(declared_local_names(&tokens, index));
        }
        let next = next_significant(&tokens, index + 1);
        let after_next = next.and_then(|(next_index, _)| next_significant(&tokens, next_index + 1));
        if at_top_level
            && is_word(token, "function")
            && next.is_some_and(|(_, token)| word(token) == Some(name))
            && after_next.is_some_and(|(_, token)| is_symbol(Some(token), b'('))
            && !top_level_locals.contains(name)
        {
            return true;
        }
        let equals = next.filter(|(_, token)| is_symbol(Some(token), b'='));
        let function = equals
            .and_then(|(equals_index, _)| next_significant(&tokens, equals_index + 1))
            .filter(|(_, token)| is_word(token, "function"));
        let open_parenthesis =
            function.and_then(|(function_index, _)| next_significant(&tokens, function_index + 1));
        if at_top_level
            && word(token) == Some(name)
            && equals.is_some()
            && function.is_some()
            && open_parenthesis.is_some_and(|(_, token)| is_symbol(Some(token), b'('))
            && !top_level_locals.contains(name)
            && !previous_significant(&tokens, index)
                .is_some_and(|token| matches!(token, Token::Symbol(b'.' | b':')))
        {
            return true;
        }

        match token {
            Token::Symbol(b'{') => table_depth += 1,
            Token::Symbol(b'}') => table_depth = table_depth.saturating_sub(1),
            _ => {}
        }
        match word_at(&tokens, index) {
            Some("function") => blocks.push(BlockEnd::End),
            Some("for" | "while") => blocks.push(BlockEnd::AwaitDo),
            Some("do") if blocks.last() == Some(&BlockEnd::AwaitDo) => {
                *blocks.last_mut().expect("matched pending loop") = BlockEnd::End;
            }
            Some("do") => blocks.push(BlockEnd::End),
            Some("then") if elseif_pending => elseif_pending = false,
            Some("then") => blocks.push(BlockEnd::End),
            Some("elseif") => elseif_pending = true,
            Some("repeat") => blocks.push(BlockEnd::Until),
            Some("end") if blocks.last() == Some(&BlockEnd::End) => {
                blocks.pop();
            }
            Some("until") if blocks.last() == Some(&BlockEnd::Until) => {
                blocks.pop();
            }
            _ => {}
        }
    }
    false
}

fn declared_local_names(tokens: &[Token], local_index: usize) -> Vec<String> {
    let Some((mut cursor, first)) = next_significant(tokens, local_index + 1) else {
        return Vec::new();
    };
    if is_word(first, "function") {
        return next_significant(tokens, cursor + 1)
            .and_then(|(_, token)| word(token))
            .map(|name| vec![name.to_string()])
            .unwrap_or_default();
    }
    let mut names = Vec::new();
    while let Some(name) = word_at(tokens, cursor) {
        names.push(name.to_string());
        let Some((separator_index, separator)) = next_significant(tokens, cursor + 1) else {
            break;
        };
        if !is_symbol(Some(separator), b',') {
            break;
        }
        let Some((name_index, _)) = next_significant(tokens, separator_index + 1) else {
            break;
        };
        cursor = name_index;
    }
    names
}

fn word_at(tokens: &[Token], index: usize) -> Option<&str> {
    tokens.get(index).and_then(word)
}

fn word(token: &Token) -> Option<&str> {
    match token {
        Token::Word(word) => Some(word),
        _ => None,
    }
}

fn next_significant(tokens: &[Token], start: usize) -> Option<(usize, &Token)> {
    tokens
        .iter()
        .enumerate()
        .skip(start)
        .find(|(_, token)| !matches!(token, Token::Newline))
}

fn previous_significant(tokens: &[Token], end: usize) -> Option<&Token> {
    tokens[..end]
        .iter()
        .rev()
        .find(|token| !matches!(token, Token::Newline))
}

fn is_word(token: &Token, expected: &str) -> bool {
    matches!(token, Token::Word(word) if word == expected)
}

fn is_symbol(token: Option<&Token>, expected: u8) -> bool {
    matches!(token, Some(Token::Symbol(actual)) if *actual == expected)
}

fn tokenize(source: &[u8]) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < source.len() {
        match source[index] {
            b'\n' | b'\r' => {
                tokens.push(Token::Newline);
                index += 1;
            }
            byte if byte.is_ascii_whitespace() => index += 1,
            b'-' if source.get(index + 1) == Some(&b'-') => {
                index += 2;
                if let Some((equals, content)) = long_bracket_open(source, index) {
                    index = skip_long_bracket(source, content, equals);
                } else {
                    while index < source.len() && !matches!(source[index], b'\n' | b'\r') {
                        index += 1;
                    }
                }
            }
            quote @ (b'\'' | b'"') => {
                index += 1;
                while index < source.len() {
                    if source[index] == b'\\' {
                        index = (index + 2).min(source.len());
                    } else if source[index] == quote {
                        index += 1;
                        break;
                    } else {
                        index += 1;
                    }
                }
            }
            b'[' => {
                if let Some((equals, content)) = long_bracket_open(source, index) {
                    index = skip_long_bracket(source, content, equals);
                } else {
                    tokens.push(Token::Symbol(b'['));
                    index += 1;
                }
            }
            byte if byte == b'_' || byte.is_ascii_alphabetic() => {
                let start = index;
                index += 1;
                while index < source.len()
                    && (source[index] == b'_' || source[index].is_ascii_alphanumeric())
                {
                    index += 1;
                }
                tokens.push(Token::Word(
                    String::from_utf8_lossy(&source[start..index]).into_owned(),
                ));
            }
            symbol @ (b'=' | b'(' | b')' | b'.' | b':' | b',' | b';' | b'{' | b'}') => {
                tokens.push(Token::Symbol(symbol));
                index += 1;
            }
            _ => index += 1,
        }
    }
    tokens
}

fn long_bracket_open(source: &[u8], start: usize) -> Option<(usize, usize)> {
    if source.get(start) != Some(&b'[') {
        return None;
    }
    let mut cursor = start + 1;
    while source.get(cursor) == Some(&b'=') {
        cursor += 1;
    }
    (source.get(cursor) == Some(&b'[')).then_some((cursor - start - 1, cursor + 1))
}

fn skip_long_bracket(source: &[u8], mut index: usize, equals: usize) -> usize {
    while index < source.len() {
        if source[index] == b']' {
            let equals_end = index + 1 + equals;
            if equals_end < source.len()
                && source[index + 1..equals_end]
                    .iter()
                    .all(|byte| *byte == b'=')
                && source[equals_end] == b']'
            {
                return equals_end + 1;
            }
        }
        index += 1;
    }
    source.len()
}
