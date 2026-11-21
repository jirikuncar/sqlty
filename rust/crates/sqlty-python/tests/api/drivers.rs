use sqlty_python::drivers::*;
use sqlty_python::module::facts;

fn calls(source: &str) -> Vec<DriverCall> {
    facts(source).unwrap().driver_calls
}

#[test]
fn sqlite_tuple() {
    let found =
        calls("import sqlite3\nconn = sqlite3.connect('db')\nconn.execute('SELECT ?', (1,))\n");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].line, 3);
    assert_eq!(
        found[0].params,
        Some(vec![Param {
            key: Key::Index(1),
            kind: Some("int".into())
        }])
    );
}

#[test]
fn cursor_of_awaited_connection() {
    let found = calls(
        "import psycopg\nasync def f():\n    conn = await psycopg.AsyncConnection.connect('')\n    cur = conn.cursor()\n    q = 'SELECT %(a)s'\n    await cur.execute(q, {'a': -1.5})\n",
    );
    assert_eq!(found[0].driver, "psycopg");
    assert_eq!(found[0].query, "SELECT %(a)s");
    assert_eq!(
        found[0].params,
        Some(vec![Param {
            key: Key::Name("a".into()),
            kind: Some("float".into())
        }])
    );
}

#[test]
fn rebound_receiver_is_unknown() {
    assert!(calls(
        "import sqlite3\nc = sqlite3.connect('')\nfor c in x: pass\nc.execute('SELECT 1')\n"
    )
    .is_empty());
}
