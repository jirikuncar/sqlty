//! The `sqlty` command line: `rewrite`, `strip`, `format`, `check`, `rule`, `skill` and
//! `version`, on the files of a [`Host`].

#![cfg_attr(coverage_nightly, feature(coverage_attribute), allow(unused_features))]

mod breaking;
mod complete;
pub mod native;
pub mod server;

use std::ffi::OsString;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use clap::{ArgMatches, Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use sqlty_project::{FileSettings, Flags, Options, Shared, Workspace};
use sqlty_python::checker::Diagnostic;
use sqlty_python::project::{Discovery, Files, Read};
use sqlty_python::report;
use sqlty_python::rules::{Rule, Severity};
use sqlty_python::types::stdlib::Version;

include!(concat!(env!("OUT_DIR"), "/embedded.rs"));

/// `check` reported an error, or `--check`/`--diff` found files to change.
pub const EXIT_FOUND: i32 = 1;
/// Bad usage or configuration, a missing path, a file that is not UTF-8, or a crash.
pub const EXIT_ERROR: i32 = 2;

const DOCS_URL: &str = "https://sqlty.kuncar.dev/";

/// Where the command line runs: its streams, its files and the commands it may start.
pub trait Host {
    /// Write to standard output.
    fn out(&mut self, text: &str);
    /// Write to standard error.
    fn err(&mut self, text: &str);
    /// Report a warning, as Python's `logging` does.
    fn warn(&mut self, message: &str);
    /// Report an error, as Python's `logging` does.
    fn error(&mut self, message: &str);
    /// The files commands read.
    fn files(&self) -> Arc<dyn Files + Send + Sync>;
    /// Replace the file at `path`.
    fn write(&mut self, path: &Path, data: &[u8]) -> Result<(), String>;
    /// Run `command`; whether it succeeded, and its standard error.
    fn run(&mut self, command: &[String]) -> Result<(bool, String), String>;
    /// The current directory, absolute.
    fn cwd(&self) -> PathBuf;
    /// An environment variable.
    fn env(&self, name: &str) -> Option<String>;
    /// The Python the sources are read as.
    fn python(&self) -> Version;
    /// Where `[[tool.sqlty.functions]]` modules are also looked for: `sys.path`.
    fn extra_roots(&self) -> Vec<PathBuf>;
    /// The version of sqlty.
    fn version(&self) -> String;
}

#[derive(Parser)]
#[command(
    name = "sqlty",
    about = "Type-safe SQL queries for Python.",
    disable_version_flag = true
)]
struct Cli {
    /// Print version and exit
    #[arg(long)]
    version: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Rewrite "--sql\n..." strings into SQL[T]("--sql\n...") calls
    Rewrite {
        #[command(flatten)]
        common: Common,
        #[command(flatten)]
        change: Change,
        /// Row-returning queries of unknown row type: leave unwrapped (skip, default) or wrap
        /// as `SQL[UNTYPED](...)` (sentinel). Config: untyped in [tool.sqlty]
        #[arg(long, value_parser = ["skip", "sentinel"])]
        untyped: Option<String>,
    },
    /// Strip SQL[T]("--sql\n...") wrappers back to plain strings
    Strip {
        #[command(flatten)]
        common: Common,
        #[command(flatten)]
        change: Change,
    },
    /// Lay out query strings with sqruff: one plain triple-quoted literal per query
    Format {
        #[command(flatten)]
        common: Common,
        #[command(flatten)]
        change: Change,
    },
    /// Lint: validate SQL type annotations and detect issues
    Check {
        #[command(flatten)]
        common: Common,
        #[command(flatten)]
        check: CheckArgs,
    },
    /// List the table and column names that complete at FILE:LINE:COL in a query string
    /// (sqlty complete app.py:12:30), for scripts and agents
    Complete {
        /// The cursor: a Python file, a 1-based line and a 1-based column in characters
        #[arg(value_name = "FILE:LINE:COL", value_parser = complete::location)]
        location: complete::Location,
        #[command(flatten)]
        common: Common,
        /// Print a JSON object with the start and end the completion replaces, whether the
        /// list is incomplete, and the items with their label, kind, detail, SQL type and
        /// nullability, instead of one item per line
        #[arg(long)]
        json: bool,
    },
    /// Check the queries and schema of two revisions across, as during a rolling deploy:
    /// sqlty breaking --against origin/main
    Breaking(breaking::Args),
    /// Explain a lint rule (sqlty rule stale-type), or list all rules
    Rule {
        /// Rule name, for example select-star
        name: Option<String>,
    },
    /// Print an Agent Skill for coding agents, or install them all
    Skill {
        /// Skill to print (default: sqlty)
        #[arg(default_value = "sqlty")]
        name: String,
        /// Write every skill to DIR/NAME/ instead of printing one
        #[arg(long, value_name = "DIR", conflicts_with = "list")]
        install: Option<PathBuf>,
        /// List the skills and what each is for
        #[arg(long)]
        list: bool,
    },
    /// Run the language server on stdin and stdout, for editors (diagnostics and fixes as
    /// you type)
    Server,
    /// Print version and exit
    Version,
}

#[derive(Args)]
struct Common {
    /// Files or directories (default: .); each file uses its nearest sqlty.toml,
    /// pyproject.toml or Cargo.toml. A project's config or schema file stands for the whole
    /// project
    paths: Vec<PathBuf>,
    /// SQL dialect: postgres, mysql, sqlite, or another sqlparser dialect such as duckdb
    ///
    /// Default: [tool.sqlty] dialect, else postgres
    #[arg(long, value_name = "DIALECT", value_parser = dialect)]
    dialect: Option<String>,
    /// Schema .sql file, directory or glob for every path; repeat for several
    ///
    /// Default: [tool.sqlty] schema per project
    #[arg(long)]
    schema: Vec<String>,
    /// Use this file for every path instead of the nearest config file. Env: SQLTY_CONFIG_FILE
    ///
    /// The file is [tool.sqlty] of a pyproject.toml, [package.metadata.sqlty] or
    /// [workspace.metadata.sqlty] of a Cargo.toml, or top-level keys of another file such
    /// as sqlty.toml; its relative paths are relative to the current directory
    #[arg(long, value_name = "PATH", conflicts_with = "isolated")]
    config_file: Option<PathBuf>,
    /// Ignore every configuration file; use the defaults and command-line flags
    #[arg(long)]
    isolated: bool,
    /// Override a configuration key with a TOML assignment; repeat for several
    ///
    /// For example `-c 'dialect = "mysql"'` or `-c 'rules.select-star = "ignore"'`. Paths
    /// are relative to the current directory
    #[arg(short = 'c', long = "config", value_name = "KEY=VALUE")]
    config: Vec<String>,
    /// Treat RULE as severity error; repeat for several, the last flag for a rule wins
    #[arg(long, value_name = "RULE")]
    error: Vec<String>,
    /// Treat RULE as severity warn; repeat for several, the last flag for a rule wins
    #[arg(long, value_name = "RULE")]
    warn: Vec<String>,
    /// Treat RULE as severity ignore; repeat for several, the last flag for a rule wins
    #[arg(long, value_name = "RULE")]
    ignore: Vec<String>,
    /// Skip this glob while walking directories, replacing the other excludes
    ///
    /// It replaces [tool.sqlty] exclude and the default excludes; repeat for several.
    /// Relative to the current directory
    #[arg(long, value_name = "GLOB")]
    exclude: Option<Vec<String>>,
    /// Skip this glob in addition to the other excludes
    ///
    /// Repeat for several. Relative to the current directory
    #[arg(long, value_name = "GLOB")]
    extend_exclude: Vec<String>,
    /// Apply the excludes to files and directories named on the command line too
    #[arg(long)]
    force_exclude: bool,
    /// Skip the files .gitignore ignores while walking directories
    ///
    /// Default: [tool.sqlty] respect-gitignore, else true
    #[arg(long, overrides_with = "no_respect_gitignore")]
    respect_gitignore: bool,
    /// Walk the files .gitignore ignores too
    #[arg(long, overrides_with = "respect_gitignore")]
    no_respect_gitignore: bool,
    /// Print each file that rewrite, strip or check --fix changes
    ///
    /// With check, also the types of each named query of a query file
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
    /// Suppress non-error output
    #[arg(short, long)]
    quiet: bool,
}

#[derive(Args)]
struct Change {
    /// Do not modify files; print what would change and exit 1 if any file would
    #[arg(long)]
    check: bool,
    /// Print a unified diff instead of modifying files; exit 1 if any file would change
    #[arg(long)]
    diff: bool,
}

#[derive(Args)]
struct CheckArgs {
    /// Apply the fixable rules (see `sqlty rule`), write the files, then report what remains
    #[arg(long)]
    fix: bool,
    /// Exit 0 even when an error-severity diagnostic is reported
    #[arg(long)]
    exit_zero: bool,
    /// Exit 0 when no diagnostic has severity error (the default)
    ///
    /// Cannot be combined with --error-on-warning
    #[arg(long)]
    exit_zero_on_warning: bool,
    /// Exit 1 when any diagnostic has severity warn
    ///
    /// Cannot be combined with --exit-zero or --exit-zero-on-warning
    #[arg(long)]
    error_on_warning: bool,
    /// After the summary, print how many queries were typed, partially typed, unparsed
    /// and skipped
    #[arg(long)]
    statistics: bool,
    /// Under a type diagnostic, print the inference rule behind each column's type
    ///
    /// The rule's id, reference and reasoning, for the type and the nullability
    #[arg(long)]
    explain: bool,
    /// How diagnostics are printed
    ///
    /// text: file:line: [rule] message (the default); json: an array of objects with file,
    /// line, rule, severity, message, the start and end of what it underlines, and the fix
    /// edits; github: GitHub Actions annotations; sarif: a SARIF 2.1.0 log, for GitHub code
    /// scanning. Summaries, --statistics and --explain are text only
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    output_format: OutputFormat,
    /// Print the files sqlty would check, and exit
    #[arg(long, conflicts_with = "show_settings")]
    show_files: bool,
    /// Print the settings of the first file sqlty would check, and exit
    #[arg(long)]
    show_settings: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
    Github,
    Sarif,
}

/// `check --show-settings`: what `path` is checked with, as TOML.
fn show_settings(path: &Path, settings: &FileSettings) -> String {
    let quote = |text: &str| toml::Value::String(text.to_string()).to_string();
    let list = |items: Vec<String>| format!("[{}]", items.join(", "));
    let table = |pairs: Vec<(&String, &String)>| {
        let pairs: Vec<String> = pairs
            .into_iter()
            .map(|(key, value)| format!("{} = {}", quote(key), quote(value)))
            .collect();
        match pairs.is_empty() {
            true => "{}".to_string(),
            false => format!("{{ {} }}", pairs.join(", ")),
        }
    };
    let mut rules: Vec<_> = settings.rules.iter().collect();
    rules.sort();
    let types = settings
        .types
        .iter()
        .map(|(key, value)| (key, value))
        .collect();
    let mut coerce: Vec<String> = settings.coerce.iter().map(|name| quote(name)).collect();
    coerce.sort();
    let functions = settings
        .functions
        .iter()
        .map(|function| quote(&function.import_path))
        .collect();
    let mut lines = vec![format!("# {}", path.display())];
    if let Some(base) = &settings.base {
        lines.push(format!("base = {}", quote(&base.display().to_string())));
    }
    lines.push(format!("dialect = {}", quote(settings.dialect)));
    lines.push(format!("schema = {}", settings.schema.is_some()));
    if let Some(style) = &settings.import_style {
        lines.push(format!("import-style = {}", quote(style)));
    }
    let untyped = if settings.untyped { "sentinel" } else { "skip" };
    lines.push(format!("untyped = {}", quote(untyped)));
    lines.push(format!("param-types = {}", settings.param_types));
    lines.push(format!("closed-params = {}", settings.closed_params));
    lines.push(format!("rules = {}", table(rules)));
    lines.push(format!("types = {}", table(types)));
    lines.push(format!("coerce-row-types = {}", list(coerce)));
    lines.push(format!("functions = {}", list(functions)));
    let queries = settings.queries.iter().map(|glob| quote(glob)).collect();
    lines.push(format!("queries = {}", list(queries)));
    format!("{}\n", lines.join("\n"))
}

fn dialect(name: &str) -> Result<String, String> {
    use sqlty_sql::Dialect;
    match name.parse::<Dialect>() {
        Ok(_) => Ok(name.to_lowercase()),
        Err(_) => Err(format!(
            "unknown dialect '{name}'{}; expected one of: {}",
            sqlty_python::config::suggest(name, &Dialect::NAMES),
            Dialect::NAMES.join(", ")
        )),
    }
}

/// One command of [`reference`]: its name (`sqlty` for the root), usage and options.
pub struct Reference {
    /// `sqlty`, or the name of a command.
    pub name: String,
    /// The usage line, without `Usage: `.
    pub usage: String,
    /// `(invocation, help)` of each option and positional argument.
    pub options: Vec<(String, String)>,
}

/// The root and each command of `sqlty`, for `docs/reference/cli.md`.
pub fn reference() -> Vec<Reference> {
    let mut root = Cli::command().term_width(80);
    root.build();
    let mut commands = vec![&root];
    commands.extend(
        root.get_subcommands()
            .filter(|command| command.get_name() != "help"),
    );
    commands
        .into_iter()
        .map(|command| Reference {
            name: command.get_name().to_string(),
            usage: command
                .clone()
                .render_usage()
                .to_string()
                .trim_start_matches("Usage: ")
                .to_string(),
            options: command
                .get_arguments()
                .filter(|arg| !arg.is_hide_set())
                .map(|arg| {
                    let help = arg.get_long_help().or(arg.get_help());
                    (
                        invocation(arg),
                        help.map(ToString::to_string).unwrap_or_default(),
                    )
                })
                .collect(),
        })
        .collect()
}

/// `-c, --config KEY=VALUE`, `--untyped {skip,sentinel}` or `paths ...`.
fn invocation(arg: &clap::Arg) -> String {
    let possible: Vec<String> = arg
        .get_possible_values()
        .iter()
        .map(|value| value.get_name().to_string())
        .collect();
    let value = match (possible.is_empty(), arg.get_value_names()) {
        (false, _) => format!("{{{}}}", possible.join(",")),
        (true, Some(names)) => names
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" "),
        (true, None) if arg.is_positional() => arg.get_id().to_string(),
        (true, None) => arg.get_id().as_str().to_uppercase(),
    };
    if arg.is_positional() {
        let many = matches!(arg.get_action(), clap::ArgAction::Append);
        return if many { format!("{value} ...") } else { value };
    }
    let mut names: Vec<String> = arg
        .get_short()
        .map(|short| format!("-{short}"))
        .into_iter()
        .collect();
    names.extend(arg.get_long().map(|long| format!("--{long}")));
    let names = names.join(", ");
    match arg.get_action().takes_values() {
        true => format!("{names} {value}"),
        false => names,
    }
}

/// Run `sqlty ARGS...` on `host`; the exit code. A panic is reported with a bug report link.
pub fn run(args: impl IntoIterator<Item = OsString>, host: &mut dyn Host) -> i32 {
    let args: Vec<OsString> = args.into_iter().collect();
    catch_unwind(AssertUnwindSafe(|| dispatch(&args, &mut *host))).unwrap_or_else(|payload| {
        let cause = report::cause(&*payload);
        host.err(&format!(
            "sqlty: crashed: {cause}\nThis is a bug in sqlty, please report it at {}\n",
            report::issue_url(cause)
        ));
        EXIT_ERROR
    })
}

fn dispatch(args: &[OsString], host: &mut dyn Host) -> i32 {
    let matches = match Cli::command().try_get_matches_from(args) {
        Ok(matches) => matches,
        Err(error) => {
            let text = error.render().to_string();
            match error.use_stderr() {
                true => host.err(&text),
                false => host.out(&text),
            }
            return error.exit_code();
        }
    };
    #[expect(clippy::expect_used, reason = "the matches are of the same command")]
    let cli = Cli::from_arg_matches(&matches).expect("parsed by its own command");
    let severities = matches
        .subcommand()
        .map(|(_, matches)| severities(matches))
        .unwrap_or_default();
    let result = match cli.command {
        _ if cli.version => version(host),
        None => {
            host.out(&format!("{}\n", Cli::command().render_help()));
            Ok(0)
        }
        Some(Command::Version) => version(host),
        Some(Command::Rule { name }) => Ok(rule(host, name.as_deref())),
        Some(Command::Skill {
            name,
            install,
            list,
        }) => skill(host, &name, install.as_deref(), list),
        Some(Command::Rewrite {
            common,
            change,
            untyped,
        }) => transform(
            host,
            &common,
            &change,
            severities,
            untyped,
            Transform::Rewrite,
        ),
        Some(Command::Strip { common, change }) => {
            transform(host, &common, &change, severities, None, Transform::Strip)
        }
        Some(Command::Format { common, change }) => {
            transform(host, &common, &change, severities, None, Transform::Format)
        }
        Some(Command::Check { common, check }) => self::check(host, &common, &check, severities),
        Some(Command::Complete {
            location,
            common,
            json,
        }) => complete::run(host, &location, common, severities, json),
        Some(Command::Breaking(args)) => breaking::run(host, &args),
        Some(Command::Server) => server::serve(host.python(), host.extra_roots()).map(|()| 0),
    };
    result.unwrap_or_else(|error| {
        host.err(&format!("sqlty: error: {error}\n"));
        EXIT_ERROR
    })
}

/// `--error/--warn/--ignore` in command-line order.
fn severities(matches: &ArgMatches) -> Vec<(String, String)> {
    let mut found: Vec<(usize, String, String)> = Vec::new();
    for severity in ["error", "warn", "ignore"] {
        let Ok(Some(rules)) = matches.try_get_many::<String>(severity) else {
            continue;
        };
        let indices = matches.indices_of(severity).into_iter().flatten();
        for (rule, at) in rules.zip(indices) {
            found.push((at, rule.clone(), severity.to_string()));
        }
    }
    found.sort();
    found
        .into_iter()
        .map(|(_, rule, severity)| (rule, severity))
        .collect()
}

fn version(host: &mut dyn Host) -> Result<i32, String> {
    let version = host.version();
    host.out(&format!("sqlty {version}\n"));
    Ok(0)
}

fn rule(host: &mut dyn Host, name: Option<&str>) -> i32 {
    let Some(name) = name else {
        let width = Rule::ALL
            .iter()
            .map(|rule| rule.name().len())
            .max()
            .unwrap_or(0);
        for rule in Rule::ALL {
            let severity = rule.default_severity().as_str();
            host.out(&format!(
                "{:<width$}  {severity:<5}  {}\n",
                rule.name(),
                rule.summary()
            ));
        }
        return 0;
    };
    let page = sqlty_rules::PAGES
        .iter()
        .find(|(path, _)| *path == format!("{name}.md"));
    match page.filter(|_| Rule::parse(name).is_some()) {
        Some((_, text)) => {
            host.out(&rule_doc(text));
            0
        }
        None => {
            host.error(&format!(
                "Unknown rule {}; run `sqlty rule` for the list",
                sqlty_project::repr(name)
            ));
            EXIT_ERROR
        }
    }
}

/// A rule page of `docs/reference/rules/` with its links to other pages pointing at the
/// documentation site.
fn rule_doc(text: &str) -> String {
    let link =
        sqlty_sql::regex(r"\]\((?P<up>(?:\.\./)*)(?P<page>[^)\s:]+)\.md(?P<anchor>#[^)\s]*)?\)");
    link.replace_all(text, |found: &regex::Captures<'_>| {
        let mut dir = vec!["reference", "rules"];
        dir.truncate(dir.len().saturating_sub(found["up"].len() / 3));
        dir.push(&found["page"]);
        let anchor = found.name("anchor").map_or("", |anchor| anchor.as_str());
        format!("]({DOCS_URL}{}/{anchor})", dir.join("/"))
    })
    .into_owned()
}

fn skill(
    host: &mut dyn Host,
    name: &str,
    install: Option<&Path>,
    list: bool,
) -> Result<i32, String> {
    if let Some(install) = install {
        for (path, text) in SKILLS {
            let target = install.join(path);
            host.write(&target, text.as_bytes())?;
            host.out(&format!("Wrote {}\n", pure(&target).display()));
        }
        return Ok(0);
    }
    let names: Vec<(&str, &str)> = SKILLS
        .iter()
        .filter_map(|(path, text)| Some((path.strip_suffix("/SKILL.md")?, *text)))
        .collect();
    if list {
        let width = names.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
        let description = sqlty_sql::regex(r"(?m)^description: (.*)$");
        for (skill, text) in &names {
            let summary = description.captures(text).map_or(String::new(), |found| {
                found[1]
                    .split(". ")
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('.')
                    .to_string()
            });
            host.out(&format!("{skill:<width$}  {summary}\n"));
        }
        return Ok(0);
    }
    match names.iter().find(|(skill, _)| *skill == name) {
        Some((_, text)) => {
            host.out(text);
            Ok(0)
        }
        None => {
            host.error(&format!(
                "Unknown skill {}; run `sqlty skill --list` for the list",
                sqlty_project::repr(name)
            ));
            Ok(EXIT_ERROR)
        }
    }
}

/// `path` as `pathlib` spells it: no `.` components, no trailing or doubled separators.
fn pure(path: &Path) -> PathBuf {
    let pure: PathBuf = path
        .components()
        .filter(|component| *component != Component::CurDir)
        .collect();
    match pure.as_os_str().is_empty() {
        true => PathBuf::from("."),
        false => pure,
    }
}

/// The workspace of the paths of `common`, with its flags.
fn workspace(
    host: &mut dyn Host,
    common: &Common,
    severities: Vec<(String, String)>,
    untyped: Option<String>,
    parse: bool,
) -> Result<Workspace, String> {
    let files = Shared(host.files());
    let cwd = host.cwd();
    let config_file = match &common.config_file {
        Some(file) => Some(file.clone()),
        None if !common.isolated => host
            .env("SQLTY_CONFIG_FILE")
            .filter(|env| !env.is_empty())
            .map(PathBuf::from),
        None => None,
    };
    let mut flags = Flags::new(
        &cwd,
        config_file.as_deref(),
        common.isolated,
        &common.config,
        severities,
        &files,
    )?;
    let inline = flags.take_discovery();
    if !common.schema.is_empty() {
        let entries = common
            .schema
            .iter()
            .cloned()
            .map(toml::Value::String)
            .collect();
        flags.schema = Some(toml::Value::Array(entries));
    }
    flags.dialect.clone_from(&common.dialect);
    flags.untyped = untyped;
    let strings = |value: Option<&toml::Value>| -> Option<Vec<String>> {
        let items = value?.as_array()?.iter();
        Some(
            items
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect(),
        )
    };
    let mut discovery = Discovery::new(Box::new(files.clone()), &cwd);
    if common.isolated {
        discovery = discovery.isolated();
    }
    if let Some(fixed) = flags.fixed_discovery() {
        discovery = discovery.fixed(fixed);
    }
    if let Some(exclude) = common
        .exclude
        .clone()
        .or_else(|| strings(inline.get("exclude")))
    {
        discovery = discovery.exclude(&exclude);
    }
    let mut extend = strings(inline.get("extend-exclude")).unwrap_or_default();
    extend.extend(common.extend_exclude.iter().cloned());
    discovery = discovery.extend_exclude(&extend);
    let respect = match (common.respect_gitignore, common.no_respect_gitignore) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        _ => inline
            .get("respect-gitignore")
            .and_then(toml::Value::as_bool),
    };
    if let Some(respect) = respect {
        discovery = discovery.respect_gitignore(respect);
    }
    let mut paths: Vec<PathBuf> = common.paths.iter().map(|path| pure(path)).collect();
    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }
    let options = Options {
        flags,
        discovery,
        version: host.python(),
        extra_roots: host.extra_roots(),
        force_exclude: common.force_exclude,
        parse,
    };
    let workspace = Workspace::open(&paths, files, options);
    if let Ok(workspace) = &workspace {
        warn_all(host, workspace);
    }
    workspace
}

fn warn_all(host: &mut dyn Host, workspace: &Workspace) {
    for warning in workspace.warnings() {
        host.warn(&warning);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Transform {
    Rewrite,
    Strip,
    Format,
}

impl Transform {
    fn words(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Rewrite => ("rewrite", "Rewrote", "rewritten"),
            Self::Strip => ("strip", "Stripped", "stripped"),
            Self::Format => ("format", "Formatted", "formatted"),
        }
    }
}

fn transform(
    host: &mut dyn Host,
    common: &Common,
    change: &Change,
    severities: Vec<(String, String)>,
    untyped: Option<String>,
    kind: Transform,
) -> Result<i32, String> {
    let (verb, done, participle) = kind.words();
    let mut workspace = workspace(host, common, severities, untyped, false)?;
    report_unreadable(host, &workspace);
    let files = workspace.files().to_vec();
    if files.is_empty() && workspace.unreadable().is_empty() {
        if !common.quiet {
            host.warn("No Python files found");
        }
        return Ok(0);
    }
    let mut settings: Vec<Arc<FileSettings>> = Vec::with_capacity(files.len());
    for path in &files {
        settings.push(workspace.settings(path)?);
    }
    let work: Vec<(&str, &FileSettings)> = files
        .iter()
        .zip(&settings)
        .map(|(path, settings)| (workspace.text(path).unwrap_or(""), &**settings))
        .collect();
    let quiet = common.quiet;
    let results = sqlty_project::par_map(&work, |(text, settings)| -> (String, Vec<Diagnostic>) {
        match kind {
            Transform::Rewrite => {
                let warnings = match quiet {
                    true => Vec::new(),
                    false => sqlty_project::skipped_strings(text, settings),
                };
                (sqlty_project::rewritten(text, settings), warnings)
            }
            Transform::Strip => (sqlty_project::stripped(text, settings), Vec::new()),
            Transform::Format => (sqlty_project::formatted(text, settings), Vec::new()),
        }
    });
    let mut changed = 0;
    for ((path, (original, settings)), result) in files.iter().zip(&work).zip(results) {
        let (result, warnings) = result;
        for diag in warnings {
            let (rule, message) = (diag.rule.name(), &diag.message);
            host.err(&format!(
                "{}:{}: [{rule}] {message}\n",
                path.display(),
                diag.line
            ));
        }
        if result == *original {
            continue;
        }
        changed += 1;
        if change.check {
            if !common.quiet {
                host.out(&format!("Would {verb} {}\n", path.display()));
            }
        } else if change.diff {
            let path = path.display().to_string();
            host.out(&unified_diff(&path, original, &result));
        } else {
            write_source(host, path, &result)?;
            if kind == Transform::Rewrite {
                format_command(host, path, &settings.format_command);
            }
            if common.verbose > 0 {
                host.out(&format!("{done} {}\n", path.display()));
            }
        }
    }
    let unreadable = !workspace.unreadable().is_empty();
    if change.check || change.diff {
        if changed > 0 {
            if change.check && !common.quiet {
                host.out(&format!("{changed} file(s) would be {participle}\n"));
            }
            return Ok(EXIT_FOUND);
        } else if change.check && !common.quiet && !unreadable {
            host.out("All files up to date\n");
        }
    }
    Ok(if unreadable { EXIT_ERROR } else { 0 })
}

fn report_unreadable(host: &mut dyn Host, workspace: &Workspace) {
    for (path, why) in workspace.unreadable() {
        host.error(&format!("{}: not UTF-8 ({why}); skipped", path.display()));
    }
}

/// Write `text` over the Python file `path` with its byte order mark and line endings.
fn write_source(host: &mut dyn Host, path: &Path, text: &str) -> Result<(), String> {
    let absolute = host.cwd().join(path);
    let raw = match host.files().read(&absolute) {
        Read::Text(raw) => raw,
        Read::Missing | Read::Unread => Vec::new(),
    };
    let bom = raw.starts_with(b"\xef\xbb\xbf");
    let first = raw.split(|byte| *byte == b'\n').next().unwrap_or(&[]);
    let crlf = raw.contains(&b'\n') && first.ends_with(b"\r");
    let mut data = Vec::with_capacity(text.len() + 3);
    if bom {
        data.extend_from_slice(b"\xef\xbb\xbf");
    }
    match crlf {
        true => data.extend_from_slice(text.replace('\n', "\r\n").as_bytes()),
        false => data.extend_from_slice(text.as_bytes()),
    }
    host.write(&absolute, &data)
}

/// Run `format-command` on `path`; whether it ran and succeeded.
fn format_command(host: &mut dyn Host, path: &Path, command: &[String]) -> bool {
    let Some(program) = command.first() else {
        return false;
    };
    let mut full = command.to_vec();
    full.push(path.display().to_string());
    match host.run(&full) {
        Ok((true, _)) => true,
        Ok((false, stderr)) => {
            let why = stderr.trim();
            host.warn(&format!(
                "format-command {program} failed on {}: {why}",
                path.display()
            ));
            false
        }
        Err(error) => {
            host.warn(&format!(
                "format-command {program} failed on {}: {error}",
                path.display()
            ));
            false
        }
    }
}

fn check(
    host: &mut dyn Host,
    common: &Common,
    args: &CheckArgs,
    severities: Vec<(String, String)>,
) -> Result<i32, String> {
    if args.error_on_warning && (args.exit_zero || args.exit_zero_on_warning) {
        let other = if args.exit_zero {
            "--exit-zero"
        } else {
            "--exit-zero-on-warning"
        };
        return Err(format!("--error-on-warning cannot be used with {other}"));
    }
    let mut workspace = workspace(host, common, severities, None, true)?;
    report_unreadable(host, &workspace);
    let query_files = workspace.query_files()?;
    warn_all(host, &workspace);
    let files = workspace.files().to_vec();
    if args.show_files || args.show_settings {
        let mut shown: Vec<&PathBuf> = files.iter().chain(&query_files).collect();
        shown.sort();
        if args.show_files {
            for path in shown {
                host.out(&format!("{}\n", path.display()));
            }
        } else if let Some(path) = shown.first() {
            let settings = workspace.settings(path)?;
            host.out(&show_settings(path, &settings));
            warn_all(host, &workspace);
        }
        return Ok(0);
    }
    let mut unreadable = !workspace.unreadable().is_empty();
    if files.is_empty() && query_files.is_empty() && !unreadable {
        if !common.quiet {
            host.warn("No Python files found");
        }
        return Ok(0);
    }
    let text_output = args.output_format == OutputFormat::Text;
    let mut fixed_files = 0;
    if args.fix {
        for (path, fixed) in files.iter().zip(workspace.fix_all()?) {
            if workspace.text(path) == Some(fixed.as_str()) {
                continue;
            }
            write_source(host, path, &fixed)?;
            let settings = workspace.settings(path)?;
            let mut fixed = fixed;
            if format_command(host, path, &settings.format_command) {
                let absolute = host.cwd().join(path);
                let text = Shared(host.files()).text(&absolute);
                fixed = text.map_or(fixed, |text| text.replace("\r\n", "\n"));
            }
            workspace.update(path, &fixed);
            fixed_files += 1;
            if common.verbose > 0 {
                match text_output {
                    true => host.out(&format!("Fixed {}\n", path.display())),
                    false => host.err(&format!("Fixed {}\n", path.display())),
                }
            }
        }
    }
    let mut reports: Vec<(PathBuf, Diagnostic, Arc<str>)> = Vec::new();
    for (path, found) in files.iter().zip(workspace.check_all()?) {
        let text: Arc<str> = Arc::from(workspace.text(path).unwrap_or(""));
        reports.extend(
            found
                .into_iter()
                .map(|diag| (path.clone(), diag, Arc::clone(&text))),
        );
    }
    let mut types = Vec::new();
    for path in &query_files {
        let text = match Shared(host.files()).text(&host.cwd().join(path)) {
            Some(text) => text.replace("\r\n", "\n"),
            None => {
                host.error(&format!("{}: not UTF-8; skipped", path.display()));
                unreadable = true;
                continue;
            }
        };
        let found = workspace.check_queries(path, &text)?;
        if common.verbose > 0 && text_output && !common.quiet {
            let shown = path.display();
            let found = workspace.query_types(path, &text)?;
            types.extend(found.into_iter().map(|line| format!("{shown}: {line}")));
        }
        let text: Arc<str> = Arc::from(text);
        reports.extend(
            found
                .into_iter()
                .map(|diag| (path.clone(), diag, Arc::clone(&text))),
        );
    }
    warn_all(host, &workspace);
    let (mut errors, mut warnings) = (0, 0);
    let mut shown = Vec::new();
    let mut lines = std::collections::HashMap::new();
    for (path, diag, text) in &reports {
        errors += usize::from(diag.severity == Severity::Error);
        warnings += usize::from(diag.severity == Severity::Warn);
        match args.output_format {
            OutputFormat::Json | OutputFormat::Sarif => {
                let lines = lines
                    .entry(text.as_ptr())
                    .or_insert_with(|| Lines::new(text));
                shown.push(json_diagnostic(path, diag, lines));
            }
            OutputFormat::Github => host.out(&format!("{}\n", github_annotation(path, diag))),
            OutputFormat::Text if !common.quiet => {
                let (rule, message) = (diag.rule.name(), &diag.message);
                host.out(&format!(
                    "{}:{}: [{rule}] {message}\n",
                    path.display(),
                    diag.line
                ));
                if args.explain && EXPLAINED_RULES.contains(&rule) {
                    for line in workspace.explain(path, diag.line)? {
                        host.out(&format!("{line}\n"));
                    }
                }
            }
            OutputFormat::Text => {}
        }
    }
    for line in types {
        host.out(&format!("{line}\n"));
    }
    match args.output_format {
        OutputFormat::Json => host.out(&pretty(&shown)),
        OutputFormat::Sarif => host.out(&pretty(&sarif_log(&host.version(), &shown))),
        OutputFormat::Text | OutputFormat::Github => {}
    }
    let text = text_output && !common.quiet;
    if fixed_files > 0 && text {
        host.out(&format!("Fixed {fixed_files} file(s)\n"));
    }
    if text && (errors > 0 || warnings > 0) {
        let mut parts = Vec::new();
        if errors > 0 {
            parts.push(format!("{errors} error(s)"));
        }
        if warnings > 0 {
            parts.push(format!("{warnings} warning(s)"));
        }
        host.out(&format!("Found {}\n", parts.join(", ")));
    }
    if args.statistics && text {
        host.out(&format!("{}\n", statistics(&workspace.statistics_all()?)));
    }
    Ok(
        if !args.exit_zero && (errors > 0 || (args.error_on_warning && warnings > 0)) {
            EXIT_FOUND
        } else if unreadable {
            EXIT_ERROR
        } else {
            0
        },
    )
}

/// Diagnostics about the type of a query's rows, which `--explain` traces to inference rules.
const EXPLAINED_RULES: [&str; 8] = [
    "stale-type",
    "narrow-type",
    "column-type-mismatch",
    "nullable-mismatch",
    "redundant-optional",
    "literal-mismatch",
    "redundant-literal",
    "json-shape-mismatch",
];

/// One line of query counts; skipped strings are broken down by rule.
fn statistics(modules: &[std::collections::HashMap<&'static str, usize>]) -> String {
    let count =
        |name: &str| -> usize { modules.iter().filter_map(|counts| counts.get(name)).sum() };
    let skipped: Vec<(&str, usize)> = [
        Rule::FstringSkipped,
        Rule::TstringSkipped,
        Rule::UncheckedQuery,
        Rule::DynamicQuery,
    ]
    .into_iter()
    .map(|rule| (rule.name(), count(rule.name())))
    .filter(|(_, n)| *n > 0)
    .collect();
    let detail: Vec<String> = skipped
        .iter()
        .map(|(rule, n)| format!("{rule}: {n}"))
        .collect();
    let mut line = format!(
        "Queries: {} typed, {} partially typed, {} unparsed, {} skipped",
        count("typed"),
        count("partially typed"),
        count("unparsed"),
        skipped.iter().map(|(_, n)| n).sum::<usize>()
    );
    if !detail.is_empty() {
        line.push_str(&format!(" ({})", detail.join(", ")));
    }
    line
}

/// The unified diff of `old` and `new`, both named `path`, with three lines of context.
fn unified_diff(path: &str, old: &str, new: &str) -> String {
    similar::TextDiff::from_lines(old, new)
        .unified_diff()
        .header(path, path)
        .to_string()
}

/// The 1-based line and column (in characters) of the byte `offset` of `text`.
/// The line starts of a text, so that a position costs a binary search and its own line.
struct Lines<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Self {
        let breaks = text.bytes().enumerate().filter(|&(_, byte)| byte == b'\n');
        Lines {
            text,
            starts: std::iter::once(0)
                .chain(breaks.map(|(at, _)| at + 1))
                .collect(),
        }
    }

    fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.text.len());
        let line = self.starts.partition_point(|&start| start <= offset);
        let before = &self.text.as_bytes()[self.starts[line - 1]..offset];
        let column = String::from_utf8_lossy(before).chars().count() + 1;
        Position { line, column }
    }
}

#[derive(Clone, Copy, Serialize)]
struct Position {
    line: usize,
    column: usize,
}

#[derive(Serialize)]
struct JsonEdit {
    start: Position,
    end: Position,
    replacement: String,
}

/// A row of `--output-format json`.
#[derive(Serialize)]
struct JsonDiagnostic {
    file: String,
    rule: &'static str,
    message: String,
    severity: &'static str,
    line: usize,
    start: Position,
    end: Position,
    fix: Vec<JsonEdit>,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    breaking: Option<Breaking>,
}

/// Which check of `sqlty breaking` found a row.
#[derive(Clone, Serialize)]
struct Breaking {
    direction: &'static str,
    against: String,
}

fn json_diagnostic(path: &Path, diag: &Diagnostic, lines: &Lines<'_>) -> JsonDiagnostic {
    JsonDiagnostic {
        file: path.display().to_string(),
        rule: diag.rule.name(),
        message: diag.message.clone(),
        severity: diag.severity.as_str(),
        line: diag.line,
        start: lines.position(diag.span.start),
        end: lines.position(diag.span.end),
        fix: diag
            .fix
            .iter()
            .map(|edit| JsonEdit {
                start: lines.position(edit.range.start),
                end: lines.position(edit.range.end),
                replacement: edit.replacement.clone(),
            })
            .collect(),
        breaking: None,
    }
}

/// `value` as two-space indented ASCII JSON, with a trailing newline, so a redirected
/// stdout in any encoding takes it.
fn pretty(value: &impl Serialize) -> String {
    let mut out = String::new();
    for char in serde_json::to_string_pretty(value)
        .unwrap_or_default()
        .chars()
    {
        match char.is_ascii() {
            true => out.push(char),
            false => {
                for unit in char.encode_utf16(&mut [0; 2]) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('\n');
    out
}

fn github_annotation(path: &Path, diag: &Diagnostic) -> String {
    let escape = |text: &str, property: bool| {
        let text = text
            .replace('%', "%25")
            .replace('\r', "%0D")
            .replace('\n', "%0A");
        match property {
            true => text.replace(':', "%3A").replace(',', "%2C"),
            false => text,
        }
    };
    let level = if diag.severity == Severity::Error {
        "error"
    } else {
        "warning"
    };
    let file = escape(&path.display().to_string(), true);
    let title = escape(&format!("sqlty {}", diag.rule.name()), true);
    format!(
        "::{level} file={file},line={},title={title}::{}",
        diag.line,
        escape(&diag.message, false)
    )
}

fn level(severity: &str) -> &'static str {
    match severity {
        "error" => "error",
        "warn" => "warning",
        _ => "none",
    }
}

/// The rows of `--output-format json` as one SARIF 2.1.0 run.
fn sarif_log<'a>(version: &'a str, rows: &'a [JsonDiagnostic]) -> SarifLog<'a> {
    let rules = Rule::ALL
        .iter()
        .map(|rule| SarifRule {
            id: rule.name(),
            short_description: Text {
                text: rule.summary().to_string(),
            },
            help_uri: format!("{DOCS_URL}reference/rules/{}/", rule.name()),
            default_configuration: Level {
                level: level(rule.default_severity().as_str()),
            },
        })
        .collect();
    SarifLog {
        schema: "https://json.schemastore.org/sarif-2.1.0.json",
        version: "2.1.0",
        runs: [SarifRun {
            tool: Tool {
                driver: Driver {
                    name: "sqlty",
                    version,
                    information_uri: DOCS_URL,
                    rules,
                },
            },
            column_kind: "unicodeCodePoints",
            results: rows.iter().map(sarif_result).collect(),
        }],
    }
}

fn sarif_result(row: &JsonDiagnostic) -> SarifResult {
    let uri = match Path::new(&row.file).is_absolute() {
        true => format!(
            "file://{}",
            row.file.replace('%', "%25").replace(' ', "%20")
        ),
        false => row.file.replace('\\', "/"),
    };
    let artifact = || Artifact { uri: uri.clone() };
    let fixes = match row.fix.is_empty() {
        true => Vec::new(),
        false => vec![SarifFix {
            artifact_changes: [ArtifactChange {
                artifact_location: artifact(),
                replacements: row
                    .fix
                    .iter()
                    .map(|edit| Replacement {
                        deleted_region: Region::new(edit.start, edit.end),
                        inserted_content: Text {
                            text: edit.replacement.clone(),
                        },
                    })
                    .collect(),
            }],
        }],
    };
    SarifResult {
        rule_id: row.rule,
        level: level(row.severity),
        message: Text {
            text: row.message.clone(),
        },
        locations: [Location {
            physical_location: PhysicalLocation {
                artifact_location: artifact(),
                region: Region::new(row.start, row.end),
            },
        }],
        fixes,
        properties: row.breaking.clone(),
    }
}

#[derive(Serialize)]
struct SarifLog<'a> {
    #[serde(rename = "$schema")]
    schema: &'static str,
    version: &'static str,
    runs: [SarifRun<'a>; 1],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SarifRun<'a> {
    tool: Tool<'a>,
    column_kind: &'static str,
    results: Vec<SarifResult>,
}

#[derive(Serialize)]
struct Tool<'a> {
    driver: Driver<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Driver<'a> {
    name: &'static str,
    version: &'a str,
    information_uri: &'static str,
    rules: Vec<SarifRule>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SarifRule {
    id: &'static str,
    short_description: Text,
    help_uri: String,
    default_configuration: Level,
}

#[derive(Serialize)]
struct Level {
    level: &'static str,
}

#[derive(Serialize)]
struct Text {
    text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SarifResult {
    rule_id: &'static str,
    level: &'static str,
    message: Text,
    locations: [Location; 1],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fixes: Vec<SarifFix>,
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<Breaking>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Location {
    physical_location: PhysicalLocation,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PhysicalLocation {
    artifact_location: Artifact,
    region: Region,
}

#[derive(Serialize)]
struct Artifact {
    uri: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Region {
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
}

impl Region {
    fn new(start: Position, end: Position) -> Self {
        Self {
            start_line: start.line,
            start_column: start.column,
            end_line: end.line,
            end_column: end.column,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SarifFix {
    artifact_changes: [ArtifactChange; 1],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactChange {
    artifact_location: Artifact,
    replacements: Vec<Replacement>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Replacement {
    deleted_region: Region,
    inserted_content: Text,
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn a_position_counts_lines_and_characters_before_the_offset() {
        let text = "ab\n\u{e9}t\u{1f600}\n\nx";
        let lines = Lines::new(text);
        for offset in 0..=text.len() + 1 {
            let before = &text.as_bytes()[..offset.min(text.len())];
            let line = before.iter().filter(|&&byte| byte == b'\n').count() + 1;
            let start = before
                .iter()
                .rposition(|&byte| byte == b'\n')
                .map_or(0, |at| at + 1);
            let column = String::from_utf8_lossy(&before[start..]).chars().count() + 1;
            let found = lines.position(offset);
            assert_eq!(
                (found.line, found.column),
                (line, column),
                "offset {offset}"
            );
        }
    }

    /// A host on a directory of the disk that keeps what the commands print.
    struct Fake {
        root: PathBuf,
        out: String,
        err: String,
        env: Vec<(&'static str, &'static str)>,
    }

    impl Host for Fake {
        fn out(&mut self, text: &str) {
            self.out.push_str(text);
        }

        fn err(&mut self, text: &str) {
            self.err.push_str(text);
        }

        fn warn(&mut self, message: &str) {
            self.err.push_str(&format!("warning: {message}\n"));
        }

        fn error(&mut self, message: &str) {
            self.err.push_str(&format!("error: {message}\n"));
        }

        fn files(&self) -> Arc<dyn Files + Send + Sync> {
            Arc::new(sqlty_project::Disk)
        }

        fn write(&mut self, path: &Path, data: &[u8]) -> Result<(), String> {
            std::fs::write(path, data).map_err(|error| error.to_string())
        }

        fn run(&mut self, _: &[String]) -> Result<(bool, String), String> {
            Ok((true, String::new()))
        }

        fn cwd(&self) -> PathBuf {
            self.root.clone()
        }

        fn env(&self, name: &str) -> Option<String> {
            let found = self.env.iter().find(|(key, _)| *key == name);
            found.map(|(_, value)| value.to_string())
        }

        fn python(&self) -> Version {
            (3, 14)
        }

        fn extra_roots(&self) -> Vec<PathBuf> {
            Vec::new()
        }

        fn version(&self) -> String {
            assert!(self.env("CRASH").is_none(), "boom");
            "0.0.0".to_string()
        }
    }

    fn project(name: &str, files: &[(&str, &str)]) -> Fake {
        let root = std::env::temp_dir().join(format!("sqlty-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("creates");
        for (path, text) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("creates");
            std::fs::write(path, text).expect("writes");
        }
        Fake {
            root,
            out: String::new(),
            err: String::new(),
            env: Vec::new(),
        }
    }

    fn sqlty(host: &mut Fake, args: &[&str]) -> i32 {
        let args = std::iter::once("sqlty").chain(args.iter().copied());
        run(args.map(OsString::from), host)
    }

    const CONFIG: (&str, &str) = (
        "pyproject.toml",
        "[tool.sqlty]\nschema = \"schema.sql\"\nrules = { redundant-optional = \"warn\" }\n",
    );
    const SCHEMA: (&str, &str) = (
        "schema.sql",
        "CREATE TABLE users (id INT NOT NULL, name TEXT);",
    );
    const QUERY: (&str, &str) = ("app/q.py", "q = \"--sql\\nSELECT id FROM users\"\n");

    #[test]
    fn prints_usage_and_what_clap_rejects() {
        let mut host = project("usage", &[]);
        assert_eq!(sqlty(&mut host, &[]), 0);
        assert!(
            host.out.starts_with("Type-safe SQL queries for Python."),
            "{}",
            host.out
        );
        assert_eq!(sqlty(&mut host, &["--help"]), 0);
        assert_eq!(sqlty(&mut host, &["check", "--dialect", "nope"]), 2);
        assert!(
            host.err
                .contains("invalid value 'nope' for '--dialect <DIALECT>'"),
            "{}",
            host.err
        );
        assert_eq!(sqlty(&mut host, &["complete", "q.py:1"]), 2);
        assert!(
            host.err.contains("expected FILE:LINE:COL, got 'q.py:1'"),
            "{}",
            host.err
        );
        std::fs::remove_dir_all(&host.root).expect("removes");
    }

    #[test]
    fn a_panic_prints_a_bug_report_link() {
        let mut host = project("crash", &[]);
        host.env.push(("CRASH", "1"));
        assert_eq!(sqlty(&mut host, &["version"]), EXIT_ERROR);
        assert_eq!(
            host.err,
            format!(
                "sqlty: crashed: boom\nThis is a bug in sqlty, please report it at {}\n",
                report::issue_url("boom")
            )
        );
        std::fs::remove_dir_all(&host.root).expect("removes");
    }

    #[test]
    fn walks_as_the_discovery_flags_say() {
        let ignored = ("app/.gitignore", "gen.py\n");
        let generated = ("app/gen.py", "q = \"--sql\\nSELECT nope FROM users\"\n");
        let mut host = project(
            "walk",
            &[
                CONFIG,
                SCHEMA,
                QUERY,
                ignored,
                generated,
                ("app/.git/HEAD", ""),
            ],
        );
        host.env = vec![("SQLTY_CONFIG_FILE", "")];
        for (args, found) in [
            (&["check", "--respect-gitignore"][..], 0),
            (&["check", "--no-respect-gitignore"][..], 1),
            (&["check", "--exclude", "gen.py"][..], 0),
            (&["check", "-c", "exclude = [\"gen.py\"]"][..], 0),
            (&["check", "-c", "extend-exclude = [\"gen.py\"]"][..], 0),
        ] {
            host.err.clear();
            assert_eq!(sqlty(&mut host, args), found, "{args:?}: {}", host.err);
        }
        std::fs::remove_dir_all(&host.root).expect("removes");
    }

    #[test]
    fn reports_in_every_format() {
        let fixable = (
            "app/q.py",
            "from sqlty import SQL\nq = SQL[tuple[int | None]](\"--sql\\nSELECT id FROM users\")\n",
        );
        let mut host = project("formats", &[CONFIG, SCHEMA, fixable]);
        assert_eq!(sqlty(&mut host, &["check", "--output-format", "github"]), 0);
        assert!(
            host.out.starts_with("::warning file=app/q.py,"),
            "{}",
            host.out
        );
        std::fs::remove_dir_all(&host.root).expect("removes");
        let unknown = ("app/q.py", "q = \"--sql\\nSELECT nope FROM users\"\n");
        let mut host = project("sarif", &[CONFIG, SCHEMA, unknown]);
        assert_eq!(sqlty(&mut host, &["check", "--output-format", "sarif"]), 1);
        assert!(
            host.out.contains("\"ruleId\": \"unknown-column\""),
            "{}",
            host.out
        );
        assert!(!host.out.contains("\"fixes\""), "{}", host.out);
        std::fs::remove_dir_all(&host.root).expect("removes");
    }

    #[test]
    fn checks_what_would_change_quietly() {
        let mut host = project("quiet", &[CONFIG, SCHEMA, QUERY]);
        assert_eq!(sqlty(&mut host, &["rewrite", "--check", "--quiet"]), 1);
        assert_eq!((host.out.as_str(), host.err.as_str()), ("", ""));
        std::fs::remove_dir_all(&host.root).expect("removes");
    }

    #[test]
    fn writes_a_new_file_and_links_the_docs() {
        let mut host = project("write", &[]);
        write_source(&mut host, Path::new("new.py"), "x = 1\n").expect("writes");
        let written = std::fs::read_to_string(host.root.join("new.py")).expect("reads");
        assert_eq!(written, "x = 1\n");
        assert_eq!(
            rule_doc("[a](stale-type.md) [b](../rules.md#rule-groups) [c](../../guide/untyped.md#x)"),
            format!(
                "[a]({DOCS_URL}reference/rules/stale-type/) [b]({DOCS_URL}reference/rules/#rule-groups) [c]({DOCS_URL}guide/untyped/#x)"
            )
        );
        std::fs::remove_dir_all(&host.root).expect("removes");
    }

    #[test]
    fn writes_sarif_uris_and_escapes_non_ascii() {
        let at = Position { line: 1, column: 1 };
        let row = |file: &str| JsonDiagnostic {
            file: file.to_string(),
            rule: "narrow-type",
            message: "é😀".to_string(),
            severity: "info",
            line: 1,
            start: at,
            end: at,
            fix: Vec::new(),
            breaking: None,
        };
        let found = pretty(&sarif_result(&row("/a b%.py")));
        assert!(found.contains(r#""uri": "file:///a%20b%25.py""#), "{found}");
        assert!(found.contains(r#""level": "none""#), "{found}");
        assert!(!found.contains("fixes"), "{found}");
        let found = pretty(&[row(r"app\q.py")]);
        assert!(found.contains(r#""file": "app\\q.py""#), "{found}");
        assert!(
            found.contains(r#""message": "\u00e9\ud83d\ude00""#),
            "{found}"
        );
        let found = pretty(&sarif_result(&row(r"app\q.py")));
        assert!(found.contains(r#""uri": "app/q.py""#), "{found}");
    }

    #[test]
    fn marks_a_missing_newline_at_the_end() {
        assert_eq!(unified_diff("q.py", "same\n", "same\n"), "");
        assert_eq!(
            unified_diff("q.py", "a\nb", "a\nc"),
            "--- q.py\n+++ q.py\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file\n"
        );
    }
}
