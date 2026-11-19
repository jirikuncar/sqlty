//! The `sqlty` command line: `rewrite`, `strip`, `format`, `check`, `rule`, `skill` and
//! `version`, on the files of a [`Host`].

#![cfg_attr(coverage_nightly, feature(coverage_attribute), allow(unused_features))]

mod diff;
mod json;
pub mod native;
pub mod server;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use clap::{ArgMatches, Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use sqlty_project::{FileSettings, Flags, Options, Shared, Workspace};
use sqlty_python::checker::Diagnostic;
use sqlty_python::project::{Discovery, Files, Read};
use sqlty_python::report;
use sqlty_python::rules::{Rule, Severity};
use sqlty_python::types::stdlib::Version;

use json::Json;

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
    /// Print version and exit
    Version,
}

#[derive(Args)]
struct Common {
    /// Files or directories (default: .)
    #[arg(default_value = ".")]
    paths: Vec<PathBuf>,
    /// SQL dialect: postgres, mysql, sqlite, or another sqlparser dialect such as duckdb
    #[arg(long, value_name = "DIALECT", value_parser = dialect)]
    dialect: Option<String>,
    /// Schema .sql file, directory or glob for every path; repeat for several
    #[arg(long)]
    schema: Vec<String>,
    /// Use this file for every path instead of the nearest config file. Env: SQLTY_CONFIG_FILE
    #[arg(long, value_name = "PATH", conflicts_with = "isolated")]
    config_file: Option<PathBuf>,
    /// Ignore every configuration file; use the defaults and command-line flags
    #[arg(long)]
    isolated: bool,
    /// Override a configuration key with a TOML assignment; repeat for several
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
    #[arg(long, value_name = "GLOB")]
    exclude: Option<Vec<String>>,
    /// Skip this glob in addition to the other excludes
    #[arg(long, value_name = "GLOB")]
    extend_exclude: Vec<String>,
    /// Apply the excludes to files and directories named on the command line too
    #[arg(long)]
    force_exclude: bool,
    /// Skip the files .gitignore ignores while walking directories
    #[arg(long, overrides_with = "no_respect_gitignore")]
    respect_gitignore: bool,
    /// Walk the files .gitignore ignores too
    #[arg(long, overrides_with = "respect_gitignore")]
    no_respect_gitignore: bool,
    /// Print each file that rewrite, strip or check --fix changes
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
    /// Apply the fixable rules, write the files, then report what remains
    #[arg(long)]
    fix: bool,
    /// Exit 0 even when an error-severity diagnostic is reported
    #[arg(long)]
    exit_zero: bool,
    /// Exit 0 when no diagnostic has severity error (the default)
    #[arg(long)]
    exit_zero_on_warning: bool,
    /// Exit 1 when any diagnostic has severity warn
    #[arg(long)]
    error_on_warning: bool,
    /// After the summary, print how many queries were typed, partially typed, unparsed
    /// and skipped
    #[arg(long)]
    statistics: bool,
    /// Under a type diagnostic, print the inference rule behind each column's type
    #[arg(long)]
    explain: bool,
    /// How diagnostics are printed
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    output_format: OutputFormat,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
    Github,
    Sarif,
}

fn dialect(name: &str) -> Result<String, String> {
    name.parse::<sqlty_sql::Dialect>()
        .map(|_| name.to_lowercase())
        .map_err(|error| error.to_string())
}

/// Run `sqlty ARGS...` on `host`; the exit code, or `None` when the command runs in Python
/// still (`complete`, `breaking` and `server`). A panic is reported with a bug report link.
pub fn run(args: impl IntoIterator<Item = OsString>, host: &mut dyn Host) -> Option<i32> {
    let args: Vec<OsString> = args.into_iter().collect();
    catch_unwind(AssertUnwindSafe(|| dispatch(&args, &mut *host))).unwrap_or_else(|payload| {
        let cause = report::cause(&*payload);
        host.err(&format!(
            "sqlty: crashed: {cause}\nThis is a bug in sqlty, please report it at {}\n",
            report::issue_url(cause)
        ));
        Some(EXIT_ERROR)
    })
}

fn dispatch(args: &[OsString], host: &mut dyn Host) -> Option<i32> {
    let first = args.get(1).and_then(|arg| arg.to_str());
    if matches!(first, Some("complete" | "breaking" | "server")) {
        return None;
    }
    let matches = match Cli::command().try_get_matches_from(args) {
        Ok(matches) => matches,
        Err(error) => {
            let text = error.render().to_string();
            match error.use_stderr() {
                true => host.err(&text),
                false => host.out(&text),
            }
            return Some(error.exit_code());
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
    };
    Some(result.unwrap_or_else(|error| {
        host.err(&format!("sqlty: error: {error}\n"));
        EXIT_ERROR
    }))
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
        let width = Rule::LISTED
            .iter()
            .map(|rule| rule.name().len())
            .max()
            .unwrap_or(0);
        for rule in Rule::LISTED {
            let severity = rule.default_severity().as_str();
            host.out(&format!(
                "{:<width$}  {severity:<5}  {}\n",
                rule.name(),
                rule.description()
            ));
        }
        return 0;
    };
    let page = RULE_PAGES
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
    let paths: Vec<PathBuf> = common.paths.iter().map(|path| pure(path)).collect();
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
            host.out(&diff::unified(&path, original, &result));
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
    for (path, diag, text) in &reports {
        errors += usize::from(diag.severity == Severity::Error);
        warnings += usize::from(diag.severity == Severity::Warn);
        match args.output_format {
            OutputFormat::Json | OutputFormat::Sarif => {
                shown.push(json_diagnostic(path, diag, text));
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
        OutputFormat::Json => host.out(&format!("{}\n", Json::Array(shown).dumps())),
        OutputFormat::Sarif => {
            host.out(&format!("{}\n", sarif_log(&host.version(), shown).dumps()));
        }
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

/// The 1-based line and column (in characters) of the byte `offset` of `text`.
fn position(text: &str, offset: usize) -> Json {
    let offset = offset.min(text.len());
    let before = &text.as_bytes()[..offset];
    let line = before.iter().filter(|byte| **byte == b'\n').count() + 1;
    let start = before
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |at| at + 1);
    let column = String::from_utf8_lossy(&before[start..]).chars().count() + 1;
    Json::object([("line", line.into()), ("column", column.into())])
}

fn json_diagnostic(path: &Path, diag: &Diagnostic, text: &str) -> Json {
    let fix = diag
        .fix
        .iter()
        .map(|edit| {
            Json::object([
                ("start", position(text, edit.range.start)),
                ("end", position(text, edit.range.end)),
                ("replacement", edit.replacement.as_str().into()),
            ])
        })
        .collect();
    Json::object([
        ("file", path.display().to_string().into()),
        ("rule", diag.rule.name().into()),
        ("message", diag.message.as_str().into()),
        ("severity", diag.severity.as_str().into()),
        ("line", diag.line.into()),
        ("start", position(text, diag.span.start)),
        ("end", position(text, diag.span.end)),
        ("fix", Json::Array(fix)),
    ])
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
fn sarif_log(version: &str, rows: Vec<Json>) -> Json {
    let rules = Rule::LISTED
        .iter()
        .map(|rule| {
            Json::object([
                ("id", rule.name().into()),
                (
                    "shortDescription",
                    Json::object([("text", rule.description().into())]),
                ),
                (
                    "helpUri",
                    format!("{DOCS_URL}reference/rules/{}/", rule.name()).into(),
                ),
                (
                    "defaultConfiguration",
                    Json::object([("level", level(rule.default_severity().as_str()).into())]),
                ),
            ])
        })
        .collect();
    let driver = Json::object([
        ("name", "sqlty".into()),
        ("version", version.into()),
        ("informationUri", DOCS_URL.into()),
        ("rules", Json::Array(rules)),
    ]);
    let results = rows.into_iter().map(sarif_result).collect();
    let run = Json::object([
        ("tool", Json::object([("driver", driver)])),
        ("columnKind", "unicodeCodePoints".into()),
        ("results", Json::Array(results)),
    ]);
    Json::object([
        (
            "$schema",
            "https://json.schemastore.org/sarif-2.1.0.json".into(),
        ),
        ("version", "2.1.0".into()),
        ("runs", Json::Array(vec![run])),
    ])
}

fn sarif_result(row: Json) -> Json {
    let Json::Object(fields) = row else {
        return Json::Null;
    };
    let fields: BTreeMap<String, Json> = fields.into_iter().collect();
    let get = |key: &str| fields.get(key).cloned().unwrap_or(Json::Null);
    let text = |key: &str| match fields.get(key) {
        Some(Json::Str(text)) => text.clone(),
        _ => String::new(),
    };
    let at = |position: &Json, key: &str| match position {
        Json::Object(fields) => fields
            .iter()
            .find(|(name, _)| name == key)
            .map_or(Json::Null, |(_, value)| value.clone()),
        _ => Json::Null,
    };
    let region = |start: &Json, end: &Json| {
        Json::object([
            ("startLine", at(start, "line")),
            ("startColumn", at(start, "column")),
            ("endLine", at(end, "line")),
            ("endColumn", at(end, "column")),
        ])
    };
    let file = text("file");
    let uri = match Path::new(&file).is_absolute() {
        true => format!("file://{}", file.replace('%', "%25").replace(' ', "%20")),
        false => file.replace('\\', "/"),
    };
    let artifact = Json::object([("uri", uri.into())]);
    let location = Json::object([
        ("artifactLocation", artifact.clone()),
        ("region", region(&get("start"), &get("end"))),
    ]);
    let mut result = vec![
        ("ruleId".to_string(), get("rule")),
        ("level".to_string(), level(&text("severity")).into()),
        (
            "message".to_string(),
            Json::object([("text", get("message"))]),
        ),
        (
            "locations".to_string(),
            Json::Array(vec![Json::object([("physicalLocation", location)])]),
        ),
    ];
    if let Some(Json::Array(fix)) = fields
        .get("fix")
        .filter(|fix| **fix != Json::Array(Vec::new()))
    {
        let replacements = fix
            .iter()
            .map(|edit| {
                let get = |key: &str| at(edit, key);
                Json::object([
                    ("deletedRegion", region(&get("start"), &get("end"))),
                    (
                        "insertedContent",
                        Json::object([("text", get("replacement"))]),
                    ),
                ])
            })
            .collect();
        let change = Json::object([
            ("artifactLocation", artifact),
            ("replacements", Json::Array(replacements)),
        ]);
        let changes = Json::object([("artifactChanges", Json::Array(vec![change]))]);
        result.push(("fixes".to_string(), Json::Array(vec![changes])));
    }
    Json::Object(result)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

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

    fn sqlty(host: &mut Fake, args: &[&str]) -> Option<i32> {
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
        assert_eq!(sqlty(&mut host, &[]), Some(0));
        assert!(
            host.out.starts_with("Type-safe SQL queries for Python."),
            "{}",
            host.out
        );
        assert_eq!(sqlty(&mut host, &["--help"]), Some(0));
        assert_eq!(sqlty(&mut host, &["check", "--dialect", "nope"]), Some(2));
        assert!(
            host.err
                .contains("invalid value 'nope' for '--dialect <DIALECT>'"),
            "{}",
            host.err
        );
        assert_eq!(sqlty(&mut host, &["server"]), None);
        std::fs::remove_dir_all(&host.root).expect("removes");
    }

    #[test]
    fn a_panic_prints_a_bug_report_link() {
        let mut host = project("crash", &[]);
        host.env.push(("CRASH", "1"));
        assert_eq!(sqlty(&mut host, &["version"]), Some(EXIT_ERROR));
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
            assert_eq!(
                sqlty(&mut host, args),
                Some(found),
                "{args:?}: {}",
                host.err
            );
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
        assert_eq!(
            sqlty(&mut host, &["check", "--output-format", "github"]),
            Some(0)
        );
        assert!(
            host.out.starts_with("::warning file=app/q.py,"),
            "{}",
            host.out
        );
        std::fs::remove_dir_all(&host.root).expect("removes");
        let unknown = ("app/q.py", "q = \"--sql\\nSELECT nope FROM users\"\n");
        let mut host = project("sarif", &[CONFIG, SCHEMA, unknown]);
        assert_eq!(
            sqlty(&mut host, &["check", "--output-format", "sarif"]),
            Some(1)
        );
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
        assert_eq!(
            sqlty(&mut host, &["rewrite", "--check", "--quiet"]),
            Some(1)
        );
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
    fn writes_sarif_of_any_row() {
        assert_eq!(sarif_result(Json::Null), Json::Null);
        let row = Json::object([("file", Json::Null), ("start", Json::Null)]);
        let Json::Object(fields) = sarif_result(row) else {
            panic!("an object");
        };
        assert_eq!(fields[1], ("level".to_string(), "none".into()));
        let row = Json::object([("file", "/a b%.py".into())]);
        let found = sarif_result(row).dumps();
        assert!(found.contains(r#""uri": "file:///a%20b%25.py""#), "{found}");
    }
}
