//! Engine-function signatures, read from a game's own `nwscript.nss`.
//!
//! An `ACTION` instruction names an engine function by number. The number is
//! the function's position among the prototypes in the `nwscript.nss` the
//! script was compiled against: the first prototype is 0, the next is 1, and
//! so on. The two games differ from id 768 on, so each game's own file is the
//! only reliable table.
//!
//! This crate ships no copy of that file. Callers read it from the game
//! install, where it lives in `data/scripts.bif`, and pass the parsed
//! [`ActionTable`] to the decompiler.

use std::fmt;

use crate::decode::Instruction;
use crate::ty::Ty;

/// One parameter of an engine function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParamSig {
    pub ty: Ty,
    /// Default value as written in `nwscript.nss`, if any.
    pub default: Option<String>,
}

/// An engine function's name, return type and parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionSig {
    pub name: String,
    pub ret: Ty,
    pub params: Vec<ParamSig>,
}

impl ActionSig {
    /// Parse one prototype, such as `int GetGlobalNumber(string sIdentifier);`.
    pub fn parse(prototype: &str) -> Result<ActionSig, String> {
        let text = prototype.trim().trim_end_matches(';').trim_end();
        let open = text
            .find('(')
            .ok_or_else(|| format!("not a function prototype: {prototype}"))?;
        if !text.ends_with(')') {
            return Err(format!("not a function prototype: {prototype}"));
        }
        let mut head = text[..open].split_whitespace();
        let (Some(ret), Some(name), None) = (head.next(), head.next(), head.next()) else {
            return Err(format!("not a function prototype: {prototype}"));
        };
        let ret =
            type_from_word(ret).ok_or_else(|| format!("unknown return type {ret} in {name}"))?;
        let params = split_params(&text[open + 1..text.len() - 1])
            .into_iter()
            .filter(|part| !part.eq_ignore_ascii_case("void"))
            .map(|part| parse_param(&part).map_err(|err| format!("{err} in {name}")))
            .collect::<Result<_, _>>()?;
        Ok(ActionSig {
            name: name.to_string(),
            ret,
            params,
        })
    }
}

/// Every engine function a script can call, indexed by `ACTION` id.
#[derive(Clone, Debug, Default)]
pub struct ActionTable {
    sigs: Vec<Option<ActionSig>>,
}

/// Why `nwscript.nss` source could not be read as an [`ActionTable`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionTableError {
    /// 1-based source line, or 0 when the problem is the file as a whole.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ActionTableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "nwscript.nss: {}", self.message)
        } else {
            write!(f, "nwscript.nss line {}: {}", self.line, self.message)
        }
    }
}

impl std::error::Error for ActionTableError {}

impl ActionTable {
    /// Read the engine functions from `nwscript.nss` source.
    ///
    /// Constants and comments are skipped. Each function prototype takes the
    /// next id, in file order, which is how the script compiler numbers them.
    pub fn parse(source: &str) -> Result<ActionTable, ActionTableError> {
        let mut table = ActionTable::default();
        let mut in_block_comment = false;
        let mut pending: Option<(usize, String)> = None;
        for (index, raw) in source.lines().enumerate() {
            let line = strip_comments(raw, &mut in_block_comment);
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (start, text) = match pending.take() {
                Some((start, mut text)) => {
                    text.push(' ');
                    text.push_str(line);
                    (start, text)
                }
                None if starts_prototype(line) => (index + 1, line.to_string()),
                None => continue,
            };
            if !text.contains(';') {
                pending = Some((start, text));
                continue;
            }
            let sig = ActionSig::parse(&text).map_err(|message| ActionTableError {
                line: start,
                message,
            })?;
            table.sigs.push(Some(sig));
        }
        if let Some((line, _)) = pending {
            return Err(ActionTableError {
                line,
                message: "function prototype is not closed with `;`".into(),
            });
        }
        if table.sigs.is_empty() {
            return Err(ActionTableError {
                line: 0,
                message: "no function prototypes found".into(),
            });
        }
        Ok(table)
    }

    /// Set the signature for one id, growing the table as needed.
    pub fn insert(&mut self, id: u16, sig: ActionSig) {
        let id = usize::from(id);
        if self.sigs.len() <= id {
            self.sigs.resize(id + 1, None);
        }
        self.sigs[id] = Some(sig);
    }

    pub fn get(&self, id: u16) -> Option<&ActionSig> {
        self.sigs.get(usize::from(id)).and_then(Option::as_ref)
    }

    pub fn name(&self, id: u16) -> Option<&str> {
        self.get(id).map(|sig| sig.name.as_str())
    }

    /// Number of ids, one past the highest.
    pub fn len(&self) -> usize {
        self.sigs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sigs.is_empty()
    }

    /// Fill in `routine_name` on every `ACTION` this table knows.
    pub fn name_actions(&self, instructions: &mut [Instruction]) {
        for ins in instructions {
            if let Some(id) = ins.routine {
                ins.routine_name = self.name(id).map(str::to_string);
            }
        }
    }
}

/// `type Name(` at the start of a line. Constants have `=` after the name.
fn starts_prototype(line: &str) -> bool {
    fn ident(s: &str) -> Option<&str> {
        let end = s
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(s.len());
        (end > 0).then(|| &s[end..])
    }
    let Some(rest) = ident(line.trim_start()) else {
        return false;
    };
    if !rest.starts_with(char::is_whitespace) {
        return false;
    }
    ident(rest.trim_start()).is_some_and(|rest| rest.trim_start().starts_with('('))
}

/// Drop `//` and `/* */` comments, leaving string literals alone.
fn strip_comments(line: &str, in_block: &mut bool) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if *in_block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                *in_block = false;
            }
            continue;
        }
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match (c, chars.peek()) {
            ('/', Some('/')) => break,
            ('/', Some('*')) => {
                chars.next();
                *in_block = true;
            }
            ('"', _) => {
                in_string = true;
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Split a parameter list on the commas between parameters. Commas inside
/// a string or a vector default (`[0.0, 0.0, 0.0]`) do not count.
fn split_params(inner: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut buf = String::new();
    let mut depth = 0usize;
    let mut in_string = false;
    for c in inner.chars() {
        match c {
            '"' => in_string = !in_string,
            '[' | '(' if !in_string => depth += 1,
            ']' | ')' if !in_string => depth = depth.saturating_sub(1),
            ',' if !in_string && depth == 0 => {
                parts.push(std::mem::take(&mut buf).trim().to_string());
                continue;
            }
            _ => {}
        }
        buf.push(c);
    }
    let last = buf.trim();
    if !last.is_empty() {
        parts.push(last.to_string());
    }
    parts.retain(|part| !part.is_empty());
    parts
}

/// `type name` or `type name = default`.
fn parse_param(part: &str) -> Result<ParamSig, String> {
    let (decl, default) = match part.split_once('=') {
        Some((decl, default)) => (decl, Some(default.trim().to_string())),
        None => (part, None),
    };
    let mut words = decl.split_whitespace();
    let (Some(ty), Some(_name), None) = (words.next(), words.next(), words.next()) else {
        return Err(format!("unreadable parameter `{part}`"));
    };
    let ty = type_from_word(ty).ok_or_else(|| format!("unknown parameter type {ty}"))?;
    Ok(ParamSig { ty, default })
}

/// NWScript type keywords. Case is ignored: TSL declares
/// `SetAvailableNPCId(INT nNPC, OBJECT_ID oidNPC)`.
fn type_from_word(word: &str) -> Option<Ty> {
    Some(match word.to_ascii_lowercase().as_str() {
        "void" => Ty::Void,
        "int" => Ty::Int,
        "float" => Ty::Float,
        "string" => Ty::Str,
        "object" | "object_id" => Ty::Object,
        "effect" => Ty::Effect,
        "event" => Ty::Event,
        "location" => Ty::Location,
        "talent" => Ty::Talent,
        "vector" => Ty::Vector,
        "action" => Ty::Action,
        _ => return None,
    })
}
