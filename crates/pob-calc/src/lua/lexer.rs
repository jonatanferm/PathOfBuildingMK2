//! Lua 5.1 lexer.

use super::interp::LuaError;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Name(String),
    Str(Vec<u8>),
    Num(f64),
    // keywords
    And,
    Break,
    Do,
    Else,
    Elseif,
    End,
    False,
    For,
    Function,
    If,
    In,
    Local,
    Nil,
    Not,
    Or,
    Repeat,
    Return,
    Then,
    True,
    Until,
    While,
    // symbols
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Hash,
    EqEq,
    NotEq,
    LtEq,
    GtEq,
    Lt,
    Gt,
    Assign,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Semi,
    Colon,
    DoubleColon,
    Comma,
    Dot,
    Concat,
    Ellipsis,
    Eof,
}

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    pub line: u32,
}

fn keyword(s: &str) -> Option<Tok> {
    Some(match s {
        "and" => Tok::And,
        "break" => Tok::Break,
        "do" => Tok::Do,
        "else" => Tok::Else,
        "elseif" => Tok::Elseif,
        "end" => Tok::End,
        "false" => Tok::False,
        "for" => Tok::For,
        "function" => Tok::Function,
        "if" => Tok::If,
        "in" => Tok::In,
        "local" => Tok::Local,
        "nil" => Tok::Nil,
        "not" => Tok::Not,
        "or" => Tok::Or,
        "repeat" => Tok::Repeat,
        "return" => Tok::Return,
        "then" => Tok::Then,
        "true" => Tok::True,
        "until" => Tok::Until,
        "while" => Tok::While,
        _ => return None,
    })
}

impl<'a> Lexer<'a> {
    #[must_use]
    pub fn new(src: &'a str) -> Self {
        let mut lx = Self {
            src: src.as_bytes(),
            pos: 0,
            line: 1,
        };
        // Skip a shebang line.
        if lx.src.starts_with(b"#") {
            while lx.pos < lx.src.len() && lx.src[lx.pos] != b'\n' {
                lx.pos += 1;
            }
        }
        lx
    }

    fn err(&self, msg: &str) -> LuaError {
        LuaError::msg(format!("line {}: {msg}", self.line))
    }

    fn peek(&self, off: usize) -> u8 {
        *self.src.get(self.pos + off).unwrap_or(&0)
    }

    /// Length of a long-bracket opener `[==[` at `pos` (0 if none): returns level + 1.
    fn long_bracket_level(&self) -> Option<usize> {
        if self.peek(0) != b'[' {
            return None;
        }
        let mut i = 1;
        while self.peek(i) == b'=' {
            i += 1;
        }
        (self.peek(i) == b'[').then_some(i - 1)
    }

    fn read_long_string(&mut self, level: usize) -> Result<Vec<u8>, LuaError> {
        self.pos += level + 2;
        // A newline right after the opener is skipped.
        if self.peek(0) == b'\r' {
            self.pos += 1;
        }
        if self.peek(0) == b'\n' {
            self.pos += 1;
            self.line += 1;
        }
        let start = self.pos;
        loop {
            if self.pos >= self.src.len() {
                return Err(self.err("unfinished long string"));
            }
            if self.src[self.pos] == b']' {
                let mut i = 1;
                while self.peek(i) == b'=' {
                    i += 1;
                }
                if i - 1 == level && self.peek(i) == b']' {
                    let s = self.src[start..self.pos].to_vec();
                    self.pos += level + 2;
                    return Ok(s);
                }
            }
            if self.src[self.pos] == b'\n' {
                self.line += 1;
            }
            self.pos += 1;
        }
    }

    fn skip_ws_and_comments(&mut self) -> Result<(), LuaError> {
        loop {
            let c = self.peek(0);
            match c {
                b'\n' => {
                    self.line += 1;
                    self.pos += 1;
                }
                b' ' | b'\t' | b'\r' | 0x0b | 0x0c => self.pos += 1,
                b'-' if self.peek(1) == b'-' => {
                    self.pos += 2;
                    if let Some(level) = self.long_bracket_level() {
                        self.read_long_string(level)?;
                    } else {
                        while self.pos < self.src.len() && self.src[self.pos] != b'\n' {
                            self.pos += 1;
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn read_string(&mut self, quote: u8) -> Result<Vec<u8>, LuaError> {
        self.pos += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = self.peek(0);
            if self.pos >= self.src.len() || c == b'\n' {
                return Err(self.err("unfinished string"));
            }
            self.pos += 1;
            if c == quote {
                break;
            }
            if c != b'\\' {
                out.push(c);
                continue;
            }
            let e = self.peek(0);
            self.pos += 1;
            match e {
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                b'r' => out.push(b'\r'),
                b'a' => out.push(7),
                b'b' => out.push(8),
                b'f' => out.push(12),
                b'v' => out.push(11),
                b'\\' => out.push(b'\\'),
                b'"' => out.push(b'"'),
                b'\'' => out.push(b'\''),
                b'\n' => {
                    self.line += 1;
                    out.push(b'\n');
                }
                d if d.is_ascii_digit() => {
                    let mut v: u32 = u32::from(d - b'0');
                    for _ in 0..2 {
                        let n = self.peek(0);
                        if n.is_ascii_digit() {
                            v = v * 10 + u32::from(n - b'0');
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                    out.push(v as u8);
                }
                other => out.push(other),
            }
        }
        Ok(out)
    }

    fn read_number(&mut self) -> Result<f64, LuaError> {
        let start = self.pos;
        if self.peek(0) == b'0' && matches!(self.peek(1), b'x' | b'X') {
            self.pos += 2;
            while self.peek(0).is_ascii_hexdigit() {
                self.pos += 1;
            }
            let s = std::str::from_utf8(&self.src[start + 2..self.pos]).unwrap_or("0");
            return u64::from_str_radix(s, 16)
                .map(|v| v as f64)
                .map_err(|_| self.err("malformed number"));
        }
        while self.peek(0).is_ascii_digit() || self.peek(0) == b'.' {
            self.pos += 1;
        }
        if matches!(self.peek(0), b'e' | b'E') {
            self.pos += 1;
            if matches!(self.peek(0), b'+' | b'-') {
                self.pos += 1;
            }
            while self.peek(0).is_ascii_digit() {
                self.pos += 1;
            }
        }
        let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("");
        s.parse::<f64>()
            .map_err(|_| self.err(&format!("malformed number '{s}'")))
    }

    pub fn next_tok(&mut self) -> Result<Tok, LuaError> {
        self.skip_ws_and_comments()?;
        if self.pos >= self.src.len() {
            return Ok(Tok::Eof);
        }
        let c = self.peek(0);
        if c.is_ascii_alphabetic() || c == b'_' {
            let start = self.pos;
            while self.peek(0).is_ascii_alphanumeric() || self.peek(0) == b'_' {
                self.pos += 1;
            }
            let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("");
            return Ok(keyword(s).unwrap_or_else(|| Tok::Name(s.to_owned())));
        }
        if c.is_ascii_digit() || (c == b'.' && self.peek(1).is_ascii_digit()) {
            return Ok(Tok::Num(self.read_number()?));
        }
        if c == b'"' || c == b'\'' {
            return Ok(Tok::Str(self.read_string(c)?));
        }
        if c == b'[' {
            if let Some(level) = self.long_bracket_level() {
                return Ok(Tok::Str(self.read_long_string(level)?));
            }
        }
        let two = (c, self.peek(1));
        let (tok, len) = match two {
            (b'=', b'=') => (Tok::EqEq, 2),
            (b'~', b'=') => (Tok::NotEq, 2),
            (b':', b':') => (Tok::DoubleColon, 2),
            (b'<', b'=') => (Tok::LtEq, 2),
            (b'>', b'=') => (Tok::GtEq, 2),
            (b'.', b'.') => {
                if self.peek(2) == b'.' {
                    (Tok::Ellipsis, 3)
                } else {
                    (Tok::Concat, 2)
                }
            }
            _ => (
                match c {
                    b'+' => Tok::Plus,
                    b'-' => Tok::Minus,
                    b'*' => Tok::Star,
                    b'/' => Tok::Slash,
                    b'%' => Tok::Percent,
                    b'^' => Tok::Caret,
                    b'#' => Tok::Hash,
                    b'<' => Tok::Lt,
                    b'>' => Tok::Gt,
                    b'=' => Tok::Assign,
                    b'(' => Tok::LParen,
                    b')' => Tok::RParen,
                    b'{' => Tok::LBrace,
                    b'}' => Tok::RBrace,
                    b'[' => Tok::LBracket,
                    b']' => Tok::RBracket,
                    b';' => Tok::Semi,
                    b':' => Tok::Colon,
                    b',' => Tok::Comma,
                    b'.' => Tok::Dot,
                    other => {
                        return Err(self.err(&format!("unexpected character '{}'", other as char)))
                    }
                },
                1,
            ),
        };
        self.pos += len;
        Ok(tok)
    }
}
