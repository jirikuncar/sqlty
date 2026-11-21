//! `NewType` arguments: a call passing one binds its query as one of the supertype does,
//! wherever the `NewType` and the types it is made of are defined. Python resolves each
//! annotation to check the model the cases are drawn from.

use proptest::prelude::*;

use crate::draw::{check, Draw};
use crate::{engine, python};

/// How a module spells `NewType`: its import or definition, the call, and whether it is
/// `typing.NewType`.
const SPELLINGS: [(&str, &str, bool); 8] = [
    ("from typing import NewType", "NewType", true),
    ("import typing", "typing.NewType", true),
    ("import typing as t", "t.NewType", true),
    ("from typing import NewType as NT", "NT", true),
    ("from typing_extensions import NewType", "NewType", true),
    ("import typing_extensions as te", "te.NewType", true),
    (
        "def NewType(name, tp):\n    return object",
        "NewType",
        false,
    ),
    (
        "from typing import NewType\n\ndef NewType(name, tp):\n    return object",
        "NewType",
        false,
    ),
];
const IMPORTS: &str = "import datetime\nfrom decimal import Decimal\nfrom uuid import UUID";
const BASES: [&str; 8] = [
    "int",
    "str",
    "float",
    "bool",
    "UUID",
    "Decimal",
    "datetime.date",
    "list[int]",
];
/// Columns, with the class of a value of each.
const COLUMNS: [(&str, &str); 5] = [
    ("n", "int"),
    ("s", "str"),
    ("u", "UUID"),
    ("f", "float"),
    ("d", "datetime.date"),
];
const SCHEMA: &str = "CREATE TABLE items (id integer PRIMARY KEY, n integer, s text, u uuid, \
                      f double precision, d date);";
const QUERIES: [&str; 4] = [
    "SELECT coalesce({c}, %(v)s) AS v FROM items",
    "INSERT INTO items (id, {c}) VALUES (1, %(v)s) RETURNING {c} AS v",
    "SELECT {c} AS v FROM items WHERE {c} = %(v)s",
    "SELECT %(v)s AS v FROM items",
];
const CONFIG: &str = "[tool.sqlty]\ndialect = \"postgres\"\nschema = \"schema.sql\"\n\n\
                      [[tool.sqlty.functions]]\nimport-path = \"db.fetch_all\"\n\
                      query-param = \"query\"\nreturn-type-param = \"row\"\n";
const DB: &str = "from typing import Any\n\n\
                  def fetch_all[T](query: str, row: type[T], /, **params: Any) -> list[T]: ...\n";

#[derive(Debug)]
struct Case {
    app: usize,
    ids: usize,
    base: &'static str,
    /// How many links of the chain `ids` defines; `app` defines the rest.
    in_ids: usize,
    links: usize,
    /// The link bound a second time, to the same supertype.
    rebound: Option<usize>,
    /// The module binds what it imports from `ids` again, to another supertype.
    shadowed: bool,
    /// The module binds the head of the annotation again in a block that never runs.
    nested: bool,
    /// `ids` and `app` are modules of a package, which imports relatively.
    package: bool,
    dotted: bool,
    call: bool,
    /// A psycopg call rather than the configured helper.
    driver: bool,
    query: &'static str,
    column: (&'static str, &'static str),
}

fn case(d: &mut Draw<'_>) -> Case {
    let links = d.int(1, 3);
    let rebound = d.bool().then(|| d.int(0, links - 1));
    Case {
        app: d.int(0, SPELLINGS.len() - 1),
        ids: d.int(0, SPELLINGS.len() - 1),
        base: d.pick(&BASES),
        in_ids: d.int(0, links),
        links,
        rebound,
        shadowed: d.int(0, 3) == 0,
        nested: d.int(0, 3) == 0,
        package: d.bool(),
        dotted: d.bool(),
        call: d.bool(),
        driver: d.bool(),
        query: d.pick(&QUERIES),
        column: d.pick(&COLUMNS),
    }
}

impl Case {
    /// The supertype of link `i` as its module spells it.
    fn supertype(&self, i: usize) -> String {
        match i {
            0 => self.base.to_string(),
            _ if i == self.in_ids && self.dotted => format!("ids.N{}", i - 1),
            _ => format!("N{}", i - 1),
        }
    }

    fn links(&self, range: std::ops::Range<usize>, spelling: usize) -> String {
        let call = SPELLINGS[spelling].1;
        let mut lines = Vec::new();
        for i in range {
            let link = format!("N{i} = {call}(\"N{i}\", {})", self.supertype(i));
            lines.push(link.clone());
            if self.rebound == Some(i) {
                lines.push(link);
            }
        }
        lines.join("\n")
    }

    fn ids(&self) -> String {
        format!(
            "{}\n{IMPORTS}\n\n{}\n",
            SPELLINGS[self.ids].0,
            self.links(0..self.in_ids, self.ids)
        )
    }

    /// The annotation of the argument.
    fn annotation(&self) -> String {
        let last = self.links - 1;
        if self.in_ids == self.links && self.dotted {
            format!("ids.N{last}")
        } else {
            format!("N{last}")
        }
    }

    /// The module calling the helper, with the argument annotated `annotation`, or not.
    fn app(&self, annotation: Option<&str>) -> String {
        let import = match (self.in_ids, self.dotted, self.package) {
            (0, _, _) => String::new(),
            (_, true, false) => "import ids".to_string(),
            (_, true, true) => "from . import ids".to_string(),
            (n, false, false) => format!("from ids import N{}", n - 1),
            (n, false, true) => format!("from .ids import N{}", n - 1),
        };
        let other = BASES[(BASES
            .iter()
            .position(|base| *base == self.base)
            .unwrap_or(0)
            + 1)
            % BASES.len()];
        let shadow = match (self.shadowed, self.in_ids, self.dotted) {
            (false, _, _) | (_, 0, _) => String::new(),
            (true, n, true) => format!("class ids:\n    N{} = {other}\n", n - 1),
            (true, n, false) => format!(
                "N{0} = {1}(\"N{0}\", {other})\n",
                n - 1,
                SPELLINGS[self.app].1
            ),
        };
        let nested = if self.nested {
            let annotation = self.annotation();
            let head = annotation.split('.').next().unwrap_or_default();
            format!("if False:\n    {head} = {other}\n")
        } else {
            String::new()
        };
        let (parameter, argument) = match (annotation, self.call) {
            (Some(annotation), false) => (format!("v: {annotation}"), "v".to_string()),
            (Some(annotation), true) => ("raw".to_string(), format!("{annotation}(raw)")),
            (None, false) => ("v".to_string(), "v".to_string()),
            (None, true) => ("raw".to_string(), "raw".to_string()),
        };
        let (column, class) = self.column;
        let query = self.query.replace("{c}", column);
        let call = if self.driver {
            let query = python::literal(&format!("--sql\n{query}"));
            format!("conn.execute(SQL[Row]({query}), {{\"v\": {argument}}})")
        } else {
            format!("fetch_all({}, Row, v={argument})", python::literal(&query))
        };
        format!(
            "{}\n{IMPORTS}\nfrom typing import NamedTuple\n\nimport psycopg\n\n\
             from db import fetch_all\nfrom sqlty import SQL\n{import}\n\n\
             {shadow}{}\n{nested}\nclass Row(NamedTuple):\n    v: {class}\n\n\
             def run(conn: psycopg.Connection, {parameter}) -> None:\n    {call}\n",
            SPELLINGS[self.app].0,
            self.links(self.in_ids..self.links, self.app),
        )
    }

    /// Whether the module binds an imported name again.
    fn shadows(&self) -> bool {
        self.shadowed && self.in_ids > 0
    }

    /// The path of module `name`.
    fn path(&self, name: &str) -> String {
        if self.package {
            format!("pkg/{name}.py")
        } else {
            format!("{name}.py")
        }
    }

    /// Whether every link is made with `typing.NewType`.
    fn real(&self) -> bool {
        (self.in_ids == 0 || SPELLINGS[self.ids].2)
            && (self.in_ids == self.links || SPELLINGS[self.app].2)
    }

    /// A program that fails unless Python resolves the annotation as [`Case::real`] says.
    fn oracle(&self) -> String {
        let package = if self.package { "pkg" } else { "" };
        let module = |name: &str, text: &str| {
            let name = match package {
                "" => name.to_string(),
                _ if name == "db" => name.to_string(),
                package => format!("{package}.{name}"),
            };
            format!(
                "_m = types.ModuleType({name:?})\n_m.__package__ = {package:?}\n\
                 sys.modules[{name:?}] = _m\nexec({}, _m.__dict__)\n",
                python::literal(text)
            )
        };
        let (annotation, base) = (self.annotation(), self.base);
        let expected = if self.real() { "True" } else { "False" };
        format!(
            "import sys, types, typing\n_p = types.ModuleType(\"pkg\")\n_p.__path__ = []\n\
             sys.modules[\"pkg\"] = _p\n{}{}{}\
             x = eval({annotation:?}, _m.__dict__)\nreal = isinstance(x, typing.NewType)\n\
             while isinstance(x, typing.NewType):\n    x = x.__supertype__\n\
             assert (real and x == eval({base:?}, _m.__dict__)) == {expected}, (real, x)\n",
            module("db", DB),
            module("ids", &self.ids()),
            module("app", &self.app(Some(&annotation))),
        )
    }
}

/// A `NewType` argument binds the query as its supertype does; one sqlty does not resolve is
/// read as an argument of unknown type, never as one of another type, and so is one the
/// module imports, or binds anywhere, again.
#[test]
fn newtype_arguments_bind_as_their_supertype() {
    check(case, |case| {
        if !case.shadows() {
            if let Err(error) = python::run(&case.oracle()) {
                return Err(TestCaseError::fail(format!("{error}\n{}", case.oracle())));
            }
        }
        let ids = case.path("ids");
        let files = [
            ("pyproject.toml", CONFIG.to_string()),
            ("schema.sql", SCHEMA.to_string()),
            ("db.py", DB.to_string()),
            ("pkg/__init__.py", String::new()),
            (ids.as_str(), case.ids()),
        ];
        let texts = [
            case.app(Some(&case.annotation())),
            case.app(Some(case.base)),
            case.app(None),
        ];
        let [newtype, supertype, unknown] =
            <[_; 3]>::try_from(engine::check_project(&files, &case.path("app"), &texts))
                .expect("three checks");
        let shown = format!("{}\n{newtype:#?}\n{supertype:#?}\n{unknown:#?}", texts[0]);
        if !case.real() || case.shadows() || case.nested {
            prop_assert_eq!(&newtype, &unknown, "{}", shown);
        } else if case.rebound.is_none() {
            prop_assert_eq!(&newtype, &supertype, "{}", shown);
        } else {
            prop_assert!(newtype == supertype || newtype == unknown, "{}", shown);
        }
        Ok(())
    });
}
