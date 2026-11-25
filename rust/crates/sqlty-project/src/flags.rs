//! What the command line sets over the configuration files: `--config-file`, `--isolated`,
//! `-c/--config`, `--error/--warn/--ignore`, `--schema`, `--dialect` and `--untyped`.

use std::path::{Path, PathBuf};

use sqlty_python::config;
use sqlty_python::project::{self, normalized, Discovery, Files, FindError, Listing};
use sqlty_python::rules::Rule;
use toml::{Table, Value};

use crate::Shared;

/// Which configuration a directory reads.
#[derive(Clone, Debug, Default)]
pub enum Configs {
    /// The nearest configuration file of each directory.
    #[default]
    Nearest,
    /// None: the defaults and the flags.
    Isolated,
    /// One file for every directory, named in messages as `label`; its paths are relative
    /// to the current directory.
    File {
        /// Its validated table.
        table: Table,
        /// How messages name it.
        label: String,
    },
}

/// The command line's settings over the configuration files.
#[derive(Clone, Debug, Default)]
pub struct Flags {
    /// Which configuration each directory reads.
    pub configs: Configs,
    /// The `-c` assignments, validated; their schema and discovery keys taken out.
    pub overrides: Table,
    /// `--error/--warn/--ignore`, the last flag of a rule winning.
    pub severities: Vec<(String, String)>,
    /// `--schema`, or `-c schema`: schema entries for every file, relative to `cwd`.
    pub schema: Option<Value>,
    /// `--dialect`.
    pub dialect: Option<String>,
    /// `--untyped`.
    pub untyped: Option<String>,
    /// The current directory, absolute.
    pub cwd: PathBuf,
}

/// A configuration a directory reads: its table, the directory its paths start from and
/// how messages name it.
pub(crate) struct Loaded {
    pub table: Table,
    pub base: Option<PathBuf>,
    pub label: String,
}

impl Flags {
    /// The flags of a command line in `cwd`: `config_file` is loaded, `assignments` (`-c`)
    /// merged and validated, `severities` checked against the rules.
    pub fn new(
        cwd: &Path,
        config_file: Option<&Path>,
        isolated: bool,
        assignments: &[String],
        severities: Vec<(String, String)>,
        files: &Shared,
    ) -> Result<Self, String> {
        let configs = match (isolated, config_file) {
            (true, _) => Configs::Isolated,
            (false, Some(file)) => Configs::File {
                table: config_file_table(file, cwd, files)?,
                label: file.display().to_string(),
            },
            (false, None) => Configs::Nearest,
        };
        let mut overrides = parse_overrides(assignments)?;
        let schema = overrides.remove("schema");
        for (rule, severity) in &severities {
            if Rule::parse(rule).is_none() {
                let names: Vec<&str> = Rule::ALL.iter().map(|rule| rule.name()).collect();
                return Err(format!(
                    "--{severity}: unknown rule '{rule}'{}; run `sqlty rule` for the list",
                    config::suggest(rule, &names)
                ));
            }
        }
        Ok(Self {
            configs,
            overrides,
            severities,
            schema,
            cwd: cwd.to_path_buf(),
            ..Self::default()
        })
    }

    /// The discovery keys of `-c`: `exclude`, `extend-exclude` and `respect-gitignore`.
    pub fn take_discovery(&mut self) -> Table {
        ["exclude", "extend-exclude", "respect-gitignore"]
            .into_iter()
            .filter_map(|key| Some((key.to_string(), self.overrides.remove(key)?)))
            .collect()
    }

    /// The discovery keys of `--config-file`.
    pub fn fixed_discovery(&self) -> Option<Table> {
        let Configs::File { table, .. } = &self.configs else {
            return None;
        };
        Some(
            ["exclude", "extend-exclude", "respect-gitignore"]
                .into_iter()
                .filter_map(|key| Some((key.to_string(), table.get(key)?.clone())))
                .collect(),
        )
    }

    /// The severities of `-c rules.X` and `--error/--warn/--ignore`, which win over every file.
    pub(crate) fn rules(&self) -> Vec<(String, String)> {
        let mut rules = config::rule_entries(self.overrides.get("rules"));
        rules.extend(self.severities.iter().cloned());
        rules
    }

    /// Where `key` of a configuration named `label` came from, for messages.
    pub(crate) fn origin(&self, key: &str, label: &str) -> String {
        if self.overrides.contains_key(key) {
            "--config".to_string()
        } else {
            label.to_string()
        }
    }

    /// The configuration of `directory`, with `-c` and the rule flags on top; `warn` takes
    /// what a configuration that is not TOML says.
    pub(crate) fn load(
        &self,
        directory: &Path,
        discovery: &Discovery,
        warn: &mut dyn FnMut(String),
    ) -> Result<Loaded, String> {
        let mut loaded = match &self.configs {
            Configs::Isolated => Loaded {
                table: Table::new(),
                base: None,
                label: "--isolated".to_string(),
            },
            Configs::File { table, label } => Loaded {
                table: table.clone(),
                base: Some(self.cwd.clone()),
                label: label.clone(),
            },
            Configs::Nearest => match discovery.find(&normalized(directory)) {
                Ok(Some(found)) => Loaded {
                    base: found.file.parent().map(Path::to_path_buf),
                    label: found.file.display().to_string(),
                    table: found.config.table.clone(),
                },
                Ok(None) => Loaded {
                    table: Table::new(),
                    base: None,
                    label: "pyproject.toml".to_string(),
                },
                Err(FindError::Syntax(error)) => {
                    warn(format!("Failed to parse {error}"));
                    Loaded {
                        table: Table::new(),
                        base: None,
                        label: "pyproject.toml".to_string(),
                    }
                }
                Err(error) => return Err(error.to_string()),
            },
        };
        for (key, value) in &self.overrides {
            if key != "rules" {
                loaded.table.insert(key.clone(), value.clone());
            }
        }
        let mut rules = match loaded.table.remove("rules") {
            Some(Value::Table(own)) => own,
            _ => Table::new(),
        };
        for (rule, value) in self.rules() {
            let value = Value::String(value);
            let entry = match rule.split_once('.') {
                Some((rule, key)) => (
                    rule.to_string(),
                    Value::Table(Table::from_iter([(key.to_string(), value)])),
                ),
                None => (rule, value),
            };
            config::merge_rules(&mut rules, Table::from_iter([entry]));
        }
        if !rules.is_empty() {
            loaded
                .table
                .insert("rules".to_string(), Value::Table(rules));
        }
        Ok(loaded)
    }
}

fn config_file_table(file: &Path, cwd: &Path, files: &Shared) -> Result<Table, String> {
    let path = if file.is_absolute() {
        file.to_path_buf()
    } else {
        cwd.join(file)
    };
    if files.text(&path).is_none() {
        let directory = !matches!(files.list(&path), Listing::Missing);
        return Err(match directory {
            true => format!("config path is not a file: {}", file.display()),
            false => format!("config file does not exist: {}", file.display()),
        });
    }
    project::load_file(&files.canonical(&path), cwd, files)
        .map(|config| config.table)
        .map_err(|error| error.to_string())
}

/// `-c/--config` TOML assignments as one table, later ones winning and tables merged key
/// by key, validated as `[tool.sqlty]` is.
pub fn parse_overrides(assignments: &[String]) -> Result<Table, String> {
    let mut merged = Table::new();
    for assignment in assignments {
        if !assignment.contains('=') {
            return Err(format!(
                "--config expects a TOML assignment such as 'dialect = \"mysql\"', got \
                 {}; use --config-file for a file",
                repr(assignment)
            ));
        }
        let table: Table = assignment.parse().map_err(|error: toml::de::Error| {
            format!(
                "--config {}: {}; quote strings, as in -c 'dialect = \"mysql\"', \
                 or use --config-file for a file",
                repr(assignment),
                error.message().trim_end()
            )
        })?;
        merge(&mut merged, table);
    }
    config::validate(merged, "--config")
        .map(|config| config.table)
        .map_err(|error| error.to_string())
}

fn merge(target: &mut Table, update: Table) {
    for (key, value) in update {
        match (target.get_mut(&key), value) {
            (Some(Value::Table(own)), Value::Table(value)) => merge(own, value),
            (_, value) => {
                target.insert(key, value);
            }
        }
    }
}

/// `text` as Python's `repr` spells a string.
pub fn repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for char in text.chars() {
        match char {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            char if char == quote => {
                out.push('\\');
                out.push(char);
            }
            char if char.is_control() => out.push_str(&format!("\\x{:02x}", char as u32)),
            char => out.push(char),
        }
    }
    out.push(quote);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overrides(assignments: &[&str]) -> Result<Table, String> {
        let assignments: Vec<String> = assignments.iter().map(ToString::to_string).collect();
        parse_overrides(&assignments)
    }

    #[test]
    fn merges_the_assignments_key_by_key() {
        let table = overrides(&[
            "rules.select-star = \"warn\"",
            "rules = { unknown-column = \"ignore\" }",
            "dialect = \"mysql\"",
            "dialect = \"sqlite\"",
        ])
        .expect("parses");
        assert_eq!(
            table.to_string(),
            "dialect = \"sqlite\"\n\n[rules]\nselect-star = \"warn\"\nunknown-column = \"ignore\"\n"
        );
    }

    #[test]
    fn a_severity_flag_keeps_the_options_of_a_rule() {
        let table = |rules: &str| -> Table { format!("[rules]\n{rules}").parse().expect("parses") };
        let flags = |table: Table, severities: &[(&str, &str)]| Flags {
            configs: Configs::File {
                table,
                label: "sqlty.toml".to_string(),
            },
            severities: severities
                .iter()
                .map(|(rule, severity)| (rule.to_string(), severity.to_string()))
                .collect(),
            ..Flags::default()
        };
        let discovery = Discovery::new(
            Box::new(Shared(std::sync::Arc::new(crate::Disk))),
            Path::new("/"),
        );
        let loaded = |flags: Flags| {
            let loaded = flags.load(Path::new("/"), &discovery, &mut |_| {});
            config::rule_entries(loaded.expect("loads").table.get("rules"))
        };
        let entries = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect()
        };
        let options = table("missing-lock-timeout = { max = \"2s\" }");
        assert_eq!(
            loaded(flags(options, &[("missing-lock-timeout", "error")])),
            entries(&[
                ("missing-lock-timeout.max", "2s"),
                ("missing-lock-timeout", "error")
            ])
        );
        assert_eq!(
            loaded(flags(
                table("missing-lock-timeout = \"ignore\""),
                &[("missing-lock-timeout.max", "1s")]
            )),
            entries(&[
                ("missing-lock-timeout", "ignore"),
                ("missing-lock-timeout.max", "1s")
            ])
        );
        assert_eq!(
            loaded(flags(Table::new(), &[("missing-lock-timeout.max", "1s")])),
            entries(&[("missing-lock-timeout.max", "1s")])
        );
    }

    #[test]
    fn reports_what_is_not_an_assignment() {
        assert_eq!(
            overrides(&["ci.toml"]).err().as_deref(),
            Some(
                "--config expects a TOML assignment such as 'dialect = \"mysql\"', got 'ci.toml'; \
                 use --config-file for a file"
            )
        );
        let error = overrides(&["dialect = mysql"]).err().unwrap_or_default();
        assert!(
            error.starts_with("--config 'dialect = mysql': ")
                && error.ends_with("; quote strings, as in -c 'dialect = \"mysql\"', or use --config-file for a file"),
            "{error}"
        );
        let error = overrides(&["dialet = \"mysql\""]).err().unwrap_or_default();
        assert!(error.contains("dialect"), "{error}");
    }

    #[test]
    fn spells_strings_as_python_does() {
        assert_eq!(repr("it's"), "\"it's\"");
        assert_eq!(repr("a'\"b"), "'a\\'\"b'");
        assert_eq!(repr("\\\n\r\t\u{7}é"), "'\\\\\\n\\r\\t\\x07é'");
    }
}
