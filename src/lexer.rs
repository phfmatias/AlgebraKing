//   AlgebraKing //
//
//   File:         [lexer.rs]
//
//   Author(s):    ['Pedro H.F Matias']
//   Site(s):      ['https://github.com/phfmatias']
//   Email(s):     ['pedrohfmatias@gmail.com']
//   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
//   Date:         ['27.10.2026']
//   Version:      ['0.0.1']
//   Status:       ['Development']
//   Language:     ['Rust']
//   Description:  ['Tokenizer for the Maple 1-D language.']

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Int(String),
    Float(String),
    Name(String),
    QName(String),
    Str(String),
    Op(&'static str),
    Quote,
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub line: usize,
    pub col: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LexError {
    pub msg: String,
    pub incomplete: bool,
}

// longest first so that the greedy match is correct
const OPS: [&str; 39] = [
    "%%%", ":=", "::", ":-", "<=", ">=", "<>", "->", "**", "..", "@@", "||", "%%", "&*", ";", ":", ",", "(", ")",
    "[", "]", "{", "}", "<", ">", "=", "+", "-", "*", "/", "^", "!", ".", "$", "@", "|", "%", "&", "~",
];

fn ends_operand(t: &Tok) -> bool {
    match t {
        Tok::Int(_) | Tok::Float(_) | Tok::Name(_) | Tok::QName(_) | Tok::Str(_) | Tok::Quote => true,
        Tok::Op(o) => matches!(*o, ")" | "]" | "}" | "!" | "%" | "%%" | "%%%" | ">"),
        Tok::Eof => false,
    }
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, LexError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out: Vec<Token> = Vec::new();
    let (mut i, mut line, mut col) = (0usize, 1usize, 1usize);
    let err = |msg: String, incomplete: bool| LexError { msg, incomplete };
    macro_rules! adv {
        ($n:expr) => {{
            for _ in 0..$n {
                if chars[i] == '\n' {
                    line += 1;
                    col = 1;
                } else {
                    col += 1;
                }
                i += 1;
            }
        }};
    }
    while i < chars.len() {
        let c = chars[i];
        let (tl, tc) = (line, col);
        if c.is_whitespace() {
            adv!(1);
            continue;
        }
        if c == '#' {
            while i < chars.len() && chars[i] != '\n' {
                adv!(1);
            }
            continue;
        }
        if c == '(' && i + 1 < chars.len() && chars[i + 1] == '*' {
            adv!(2);
            loop {
                if i + 1 >= chars.len() {
                    return Err(err("unterminated comment".into(), true));
                }
                if chars[i] == '*' && chars[i + 1] == ')' {
                    adv!(2);
                    break;
                }
                adv!(1);
            }
            continue;
        }
        let push = |out: &mut Vec<Token>, tok: Tok| out.push(Token { tok, line: tl, col: tc });
        // numbers
        let prev_operand = out.last().map(|t| ends_operand(&t.tok)).unwrap_or(false);
        let dot_number = c == '.'
            && i + 1 < chars.len()
            && chars[i + 1].is_ascii_digit()
            && !prev_operand;
        if c.is_ascii_digit() || dot_number {
            let start = i;
            let mut is_float = false;
            while i < chars.len() && chars[i].is_ascii_digit() {
                adv!(1);
            }
            if i < chars.len() && chars[i] == '.' && !(i + 1 < chars.len() && chars[i + 1] == '.') {
                is_float = true;
                adv!(1);
                while i < chars.len() && chars[i].is_ascii_digit() {
                    adv!(1);
                }
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    is_float = true;
                    let n = j - i;
                    adv!(n);
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        adv!(1);
                    }
                }
            }
            let text: String = chars[start..i].iter().collect();
            push(&mut out, if is_float { Tok::Float(text) } else { Tok::Int(text) });
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                adv!(1);
            }
            let text: String = chars[start..i].iter().collect();
            push(&mut out, Tok::Name(text));
            continue;
        }
        if c == '`' {
            adv!(1);
            let mut s = String::new();
            loop {
                if i >= chars.len() {
                    return Err(err("unterminated quoted name".into(), true));
                }
                if chars[i] == '`' {
                    if i + 1 < chars.len() && chars[i + 1] == '`' {
                        s.push('`');
                        adv!(2);
                        continue;
                    }
                    adv!(1);
                    break;
                }
                s.push(chars[i]);
                adv!(1);
            }
            push(&mut out, Tok::QName(s));
            continue;
        }
        if c == '"' {
            adv!(1);
            let mut s = String::new();
            loop {
                if i >= chars.len() {
                    return Err(err("unterminated string".into(), true));
                }
                let ch = chars[i];
                if ch == '"' {
                    if i + 1 < chars.len() && chars[i + 1] == '"' {
                        s.push('"');
                        adv!(2);
                        continue;
                    }
                    adv!(1);
                    break;
                }
                if ch == '\\' && i + 1 < chars.len() {
                    let n = chars[i + 1];
                    let mapped = match n {
                        'n' => Some('\n'),
                        't' => Some('\t'),
                        '\\' => Some('\\'),
                        '"' => Some('"'),
                        _ => None,
                    };
                    if let Some(m) = mapped {
                        s.push(m);
                        adv!(2);
                        continue;
                    }
                }
                s.push(ch);
                adv!(1);
            }
            push(&mut out, Tok::Str(s));
            continue;
        }
        if c == '\'' {
            adv!(1);
            push(&mut out, Tok::Quote);
            continue;
        }
        let mut matched = None;
        for op in OPS.iter() {
            let n = op.chars().count();
            if i + n <= chars.len() && chars[i..i + n].iter().copied().eq(op.chars()) {
                matched = Some(*op);
                break;
            }
        }
        match matched {
            Some(op) => {
                let n = op.chars().count();
                adv!(n);
                let op = if op == "**" { "^" } else { op };
                push(&mut out, Tok::Op(op));
            }
            None => {
                return Err(err(format!("invalid character `{}` at line {}, column {}", c, tl, tc), false));
            }
        }
    }
    out.push(Token { tok: Tok::Eof, line, col });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        tokenize(s).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn numbers_and_ranges() {
        assert_eq!(toks("1..3"), vec![Tok::Int("1".into()), Tok::Op(".."), Tok::Int("3".into()), Tok::Eof]);
        assert_eq!(toks("2.5"), vec![Tok::Float("2.5".into()), Tok::Eof]);
        assert_eq!(toks("2."), vec![Tok::Float("2.".into()), Tok::Eof]);
        assert_eq!(toks(".5"), vec![Tok::Float(".5".into()), Tok::Eof]);
        assert_eq!(toks("1e-3"), vec![Tok::Float("1e-3".into()), Tok::Eof]);
        assert_eq!(toks("2*e"), vec![Tok::Int("2".into()), Tok::Op("*"), Tok::Name("e".into()), Tok::Eof]);
        assert_eq!(toks("A.B"), vec![Tok::Name("A".into()), Tok::Op("."), Tok::Name("B".into()), Tok::Eof]);
    }

    #[test]
    fn operators() {
        assert_eq!(
            toks("x:=y**2;"),
            vec![
                Tok::Name("x".into()),
                Tok::Op(":="),
                Tok::Name("y".into()),
                Tok::Op("^"),
                Tok::Int("2".into()),
                Tok::Op(";"),
                Tok::Eof
            ]
        );
        assert_eq!(toks("a<>b")[1], Tok::Op("<>"));
        assert_eq!(toks("f:-g")[1], Tok::Op(":-"));
        assert_eq!(toks("x->x")[1], Tok::Op("->"));
        assert_eq!(toks("%%%")[0], Tok::Op("%%%"));
    }

    #[test]
    fn strings_names_comments() {
        assert_eq!(toks("\"a\"\"b\""), vec![Tok::Str("a\"b".into()), Tok::Eof]);
        assert_eq!(toks("`a b`"), vec![Tok::QName("a b".into()), Tok::Eof]);
        assert_eq!(toks("x # comment\n y"), vec![Tok::Name("x".into()), Tok::Name("y".into()), Tok::Eof]);
        assert_eq!(toks("x (* c *) y"), vec![Tok::Name("x".into()), Tok::Name("y".into()), Tok::Eof]);
        assert_eq!(toks("'x'"), vec![Tok::Quote, Tok::Name("x".into()), Tok::Quote, Tok::Eof]);
        assert!(tokenize("\"abc").unwrap_err().incomplete);
    }
}
