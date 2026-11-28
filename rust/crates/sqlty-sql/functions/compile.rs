//! The function catalog compiler: `functions/<dialect>.txt` to Rust source. `build.rs`
//! runs it; `tests/function_catalog_compiler.rs` tests it.
//!
//! A line is `name(parameters) -> result<TAB>page`. A parameter is `name: type` or
//! `type`, `VARIADIC` before it when it repeats; `[, ...]` holds optional parameters, and
//! `...` repeats the parameter before it. A type may hold `(...)` and `[...]` (`MAP(K, V)`,
//! `FLOAT[3]`): a `[` right after a type is part of it unless a `,` follows. `# docs: URL` sets the base of the pages.

use std::fmt::Write as _;

struct Parameter {
    name: Option<String>,
    label: (usize, usize),
    optional: bool,
}

struct Signature {
    name: String,
    label: String,
    parameters: Vec<Parameter>,
    variadic: bool,
    returns: String,
    page: String,
}

/// The Rust items of the catalog `text` of `file`, named `name`: a `Catalog` static.
pub fn compile(file: &str, name: &str, text: &str) -> Result<String, String> {
    let mut docs = None;
    let mut signatures = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let at = |message: &str| format!("{file}:{}: {message}", number + 1);
        if let Some(url) = line.strip_prefix("# docs: ") {
            docs = Some(url.to_string());
        } else if !line.is_empty() && !line.starts_with('#') {
            signatures.push(signature(line).map_err(|e| at(&e))?);
        }
    }
    let docs = docs.ok_or_else(|| format!("{file}: no `# docs:` line"))?;
    signatures.sort_by(|a, b| a.name.cmp(&b.name));
    let mut code = format!(
        "const {name}_DOCS: &str = {docs:?};\npub(crate) static {name}: Catalog = Catalog {{\n    signatures: &[\n"
    );
    for s in &signatures {
        let required = s.parameters.iter().filter(|p| !p.optional).count();
        write!(
            code,
            "        Signature {{ name: {:?}, label: {:?}, returns: {:?}, docs: {name}_DOCS, page: {:?}, required: {required}, variadic: {}, parameters: &[",
            s.name, s.label, s.returns, s.page, s.variadic
        )
        .unwrap();
        for p in &s.parameters {
            write!(
                code,
                "Parameter {{ name: {:?}, label: ({}, {}), optional: {} }}, ",
                p.name, p.label.0, p.label.1, p.optional
            )
            .unwrap();
        }
        code.push_str("] },\n");
    }
    code.push_str("    ],\n};\n");
    Ok(code)
}

fn signature(line: &str) -> Result<Signature, String> {
    let (label, page) = line.split_once('\t').ok_or("no tab before the page")?;
    if !label.is_ascii() {
        return Err("a signature must be ASCII".to_string());
    }
    let open = label.find('(').ok_or("no `(`")?;
    let name = &label[..open];
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(format!("`{name}` is not a lower-case function name"));
    }
    let (params, returns) = label[open + 1..]
        .split_once(") -> ")
        .ok_or("no `) -> ` before the result")?;
    let mut parameters = Vec::new();
    let mut variadic = false;
    let mut depth = 0usize;
    let mut start = open + 1;
    let bytes = label.as_bytes();
    let end = open + 1 + params.len();
    let mut nested = 0usize;
    let mut i = start;
    while i <= end {
        let c = if i == end { b',' } else { bytes[i] };
        let in_type = nested > 0
            || c == b'('
            || c == b'['
                && (bytes[i - 1].is_ascii_alphanumeric()
                    || matches!(bytes[i - 1], b']' | b')' | b'"'))
                && bytes.get(i + 1) != Some(&b',');
        if in_type {
            match c {
                b'(' | b'[' => nested += 1,
                b')' | b']' => nested -= 1,
                _ => {}
            }
            i += 1;
            continue;
        }
        if c == b')' {
            return Err("unbalanced `)`".to_string());
        }
        if matches!(c, b'[' | b']' | b',') {
            let item = &label[start..i];
            let trimmed = item.trim();
            if trimmed == "..." {
                variadic = true;
            } else if !trimmed.is_empty() {
                let lead = start + (item.len() - item.trim_start().len());
                let (item, lead) = match trimmed.strip_prefix("VARIADIC ") {
                    Some(rest) => {
                        variadic = true;
                        (rest, lead + "VARIADIC ".len())
                    }
                    None => (trimmed, lead),
                };
                let name = item.split_once(": ").map(|(name, _)| name.to_string());
                parameters.push(Parameter {
                    name,
                    label: (lead, lead + item.len()),
                    optional: depth > 0,
                });
            }
            match c {
                b'[' => depth += 1,
                b']' => depth = depth.checked_sub(1).ok_or("unbalanced `]`")?,
                _ => {}
            }
            start = i + 1;
        }
        i += 1;
    }
    if depth != 0 || nested != 0 {
        return Err("unbalanced `[` or `(`".to_string());
    }
    if variadic && parameters.is_empty() {
        return Err("`...` repeats no parameter".to_string());
    }
    Ok(Signature {
        name: name.to_string(),
        label: label.to_string(),
        parameters,
        variadic,
        returns: returns.to_string(),
        page: page.to_string(),
    })
}
