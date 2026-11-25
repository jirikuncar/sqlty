//! What the analyzer's public API reads: a query's diagnostics, rule by rule, the type of
//! each column, its parameters and inference rules, and what a schema defines or skips.

use sqlty_sql::analyzer::{self, Schema};

const SCHEMA: &str = "
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers, note TEXT);
CREATE TABLE order_items (id INTEGER PRIMARY KEY, order_id INTEGER NOT NULL REFERENCES orders, amount NUMERIC NOT NULL);
CREATE TABLE payments (id INTEGER PRIMARY KEY, order_id INTEGER NOT NULL REFERENCES orders, amount NUMERIC NOT NULL);
CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT UNIQUE, name TEXT NOT NULL);
CREATE TABLE tags (order_id INTEGER, label TEXT);
CREATE TABLE docs (id INTEGER PRIMARY KEY, body JSONB NOT NULL CHECK (jsonb_typeof(body) = 'object'), meta JSON);
CREATE TABLE shapes (id INTEGER PRIMARY KEY, area CIRCLE);
CREATE TABLE states (id INTEGER PRIMARY KEY, status TEXT CHECK (status IN ('open', 'shut')), level INT CHECK (level IN (1)));
CREATE FUNCTION order_count() RETURNS INT LANGUAGE sql AS 'SELECT 1';
CREATE TABLE pairs (a INTEGER REFERENCES orders, b INTEGER REFERENCES customers, PRIMARY KEY (a, b));
CREATE FUNCTION tag_count(x INT, y nope) RETURNS INT LANGUAGE sql AS 'SELECT 1';
CREATE FUNCTION all_orders() RETURNS SETOF orders LANGUAGE sql AS 'SELECT * FROM orders';
CREATE VIEW lost AS SELECT * FROM nowhere;
CREATE VIEW order_totals AS SELECT order_id, SUM(amount) AS total, unknown_fn(order_id) AS doc FROM order_items GROUP BY order_id;
";

/// `(dialect, query, diagnostics as "[rule] message")`, against `SCHEMA`.
type Case = (&'static str, &'static str, &'static [&'static str]);

#[rustfmt::skip]
const CASES: &[Case] = &[
    ("postgres", "UPDATE states SET status = 'gone', level = 2 WHERE id = 1", &["[literal-write] `states.status` is 'open' or 'shut', not 'gone'", "[literal-write] `states.level` is 1, not 2"]),
    ("sqlite", "INSERT INTO states VALUES (1, 'gone', 1)", &["[literal-write] `states.status` is 'open' or 'shut', not 'gone'"]),
    ("postgres", "INSERT INTO states (id, status) VALUES (1, 'open') ON CONFLICT (id) DO UPDATE SET status = 'gone', (level) = (2)", &["[literal-write] `states.status` is 'open' or 'shut', not 'gone'", "[literal-write] `states.level` is 1, not 2"]),
    ("postgres", "UPDATE states SET (status, level) = ('gone', '7') WHERE id = 1", &["[literal-write] `states.status` is 'open' or 'shut', not 'gone'"]),
    ("sqlite", "INSERT INTO states (id, status) SELECT 1, 'gone' UNION ALL SELECT 2, 'open' EXCEPT SELECT 3, 'lost'", &["[literal-write] `states.status` is 'open' or 'shut', not 'gone'"]),
    ("postgres", "SELECT s.id FROM states s WHERE s.status IN ('gone', 'lost') AND level = 1", &["[unmatched-literal] `s.status` is never 'gone' or 'lost', so this WHERE matches no row"]),
    ("postgres", "DELETE FROM states WHERE 2 = level", &["[unmatched-literal] `states.level` is never 2, so this WHERE matches no row"]),
    ("postgres", "UPDATE states SET id = 2 WHERE status = 'gone' OR status IN ('open', 'lost') OR level = '2'", &[]),
    ("mysql", "SELECT id FROM states WHERE status = 'gone'", &[]),
    ("postgres", "UPDATE states SET status = 'shut', level = level, id = 2 WHERE id = 1", &[]),
    ("mysql", "UPDATE states SET status = 'gone' WHERE id = 1", &[]),
    ("postgres", "SELECT o.id, SUM(i.amount), SUM(p.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p ON p.order_id = o.id GROUP BY o.id", &["[fanout-aggregate] SUM(i.amount) aggregates order_items, whose rows repeat once per payments row", "[fanout-aggregate] SUM(p.amount) aggregates payments, whose rows repeat once per order_items row", "[chasm-join] orders joins order_items and payments one-to-many, so each row of one repeats once per row of the other"]),
    ("postgres", "SELECT SUM(LENGTH(t.label)) FROM orders o JOIN tags t ON t.order_id = o.id JOIN tags u ON u.label = t.label", &["[unproven-join] the join of t and u matches no primary or unique key, so how often it repeats their rows is unknown"]),
    ("postgres", "SELECT COUNT(*) FROM orders o JOIN order_items i USING (id) GROUP BY ALL", &["[entity-mismatch] orders.id holds orders keys, order_items.id holds order_items keys"]),
    ("postgres", "SELECT o.id, SUM(i.amount) FROM orders o, order_items i, payments p WHERE (i.order_id = o.id AND p.order_id = o.id) AND 1 = o.id GROUP BY 1", &["[fanout-aggregate] SUM(i.amount) aggregates order_items, whose rows repeat once per payments row", "[chasm-join] orders joins order_items and payments one-to-many, so each row of one repeats once per row of the other"]),
    ("postgres", "SELECT o.id, MAX(i.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p ON p.order_id = o.id GROUP BY o.id, i.id", &[]),
    ("postgres", "SELECT o.id, SUM(i.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p ON p.order_id = o.id AND p.id = (SELECT MAX(id) FROM payments) GROUP BY o.id", &["[fanout-aggregate] SUM(i.amount) aggregates order_items, whose rows repeat once per payments row", "[chasm-join] orders joins order_items and payments one-to-many, so each row of one repeats once per row of the other"]),
    ("postgres", "SELECT o.id, SUM(i.amount) FROM orders o JOIN order_items i ON o.id = i.order_id JOIN payments p ON p.id = o.customer_id + 1 GROUP BY o.id", &[]),
    ("postgres", "SELECT o.id, SUM(d.total) FROM orders o JOIN (SELECT DISTINCT order_id, amount AS total FROM payments) d ON d.order_id = o.id GROUP BY o.id", &[]),
    ("postgres", "SELECT o.id, SUM(d.total) FROM orders o JOIN (SELECT DISTINCT ON (order_id) order_id, amount AS total FROM payments) d ON d.order_id = o.id GROUP BY o.id", &[]),
    ("postgres", "SELECT o.id, SUM(d.total) FROM orders o JOIN (SELECT order_id, SUM(amount) AS total FROM payments GROUP BY 1) d ON d.order_id = o.id JOIN order_items i ON i.order_id = o.id GROUP BY o.id", &["[fanout-aggregate] SUM(d.total) aggregates d, whose rows repeat once per order_items row"]),
    ("postgres", "SELECT o.id, SUM(d.total) FROM orders o JOIN (SELECT order_id AS oid, SUM(amount) AS total FROM payments GROUP BY oid) d ON d.oid = o.id GROUP BY o.id", &[]),
    ("postgres", "SELECT o.id, SUM(d.amount) FROM orders o JOIN (SELECT p.id, p.amount FROM payments p) d ON d.id = o.id GROUP BY o.id", &[]),
    ("postgres", "SELECT o.id, SUM(d.total) FROM orders o JOIN (SELECT order_id, SUM(amount) AS total FROM payments GROUP BY ROLLUP (order_id)) d ON d.order_id = o.id GROUP BY o.id", &[]),
    ("postgres", "SELECT o.id, SUM(t.total) FROM orders o JOIN order_totals t ON t.order_id = o.id GROUP BY o.id", &[]),
    ("postgres", "SELECT o.id, SUM(i.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN orders o2 ON o2.id = i.order_id GROUP BY o.id", &[]),
    ("postgres", "SELECT * FROM orders o JOIN customers o ON o.id = o.id", &["[duplicate-alias] table name `o` is specified more than once"]),
    ("postgres", "SELECT *", &["[rejected-query] `SELECT *` with no FROM selects no columns"]),
    ("sqlite", "SELECT EXTRACT(YEAR FROM '2024-01-01')", &["[rejected-query] SQLite has no EXTRACT; use strftime"]),
    ("postgres", "SELECT (o).* FROM orders o", &[]),
    ("postgres", "VALUES (1, 2) UNION ALL VALUES (3, 4)", &[]),
    ("postgres", "TABLE orders UNION TABLE orders", &[]),
    ("postgres", "SELECT * FROM order_totals", &["[unsupported-syntax] view column `order_totals.doc` is not typed by the view's query"]),
    ("postgres", "SELECT * FROM shapes", &[]),
    ("postgres", "SELECT o.* FROM orders o GROUP BY o.id", &[]),
    ("postgres", "SELECT o FROM orders o GROUP BY o.note", &["[ungrouped-column] column `o.*` must appear in GROUP BY or be used in an aggregate function"]),
    ("postgres", "SELECT * FROM orders GROUP BY note", &["[ungrouped-column] column `orders.id` must appear in GROUP BY or be used in an aggregate function", "[ungrouped-column] column `orders.customer_id` must appear in GROUP BY or be used in an aggregate function"]),
    ("postgres", "SELECT c.* FROM orders o JOIN customers c ON c.id = o.customer_id GROUP BY o.id", &["[ungrouped-column] column `c.id` must appear in GROUP BY or be used in an aggregate function", "[ungrouped-column] column `c.email` must appear in GROUP BY or be used in an aggregate function", "[ungrouped-column] column `c.name` must appear in GROUP BY or be used in an aggregate function"]),
    ("postgres", "INSERT INTO tags VALUES (1, 'a', 2)", &["[insert-value-count] INSERT into `tags` gives 3 values for 2 columns"]),
    ("postgres", "INSERT INTO tags (order_id) VALUES (1, 2)", &["[insert-value-count] INSERT into `tags` gives 2 values for 1 column"]),
    ("postgres", "INSERT INTO tags (order_id, label) SELECT id FROM orders", &["[insert-value-count] INSERT into `tags` gives 1 value for 2 columns"]),
    ("postgres", "UPDATE orders SET (note, customer_id) = ('x', 1) WHERE id = 1 RETURNING *", &[]),
    ("postgres", "UPDATE orders o SET note = c.name FROM customers c WHERE c.id = o.customer_id AND c.nope = 1", &["[unknown-column] column `nope` does not exist in `c`"]),
    ("postgres", "DELETE FROM orders o USING customers c WHERE c.id = o.customer_id RETURNING o.*", &[]),
    ("postgres", "INSERT INTO orders (id, customer_id) VALUES (1, 2) ON CONFLICT (id) DO UPDATE SET (note, customer_id) = (EXCLUDED.note, 3)", &[]),
    ("postgres", "SELECT * FROM orders o JOIN customers c ON c.id = o.id", &["[entity-mismatch] customers.id holds customers keys, orders.id holds orders keys"]),
    ("postgres", "SELECT * FROM orders o JOIN payments p ON p.order_id = o.customer_id", &["[foreign-key-mismatch] payments.order_id references orders.id, not orders.customer_id", "[entity-mismatch] payments.order_id holds orders keys, orders.customer_id holds customers keys"]),
    ("postgres", "SELECT * FROM orders o WHERE o.customer_id IN (SELECT id FROM payments)", &["[foreign-key-mismatch] orders.customer_id references customers.id, not payments.id", "[entity-mismatch] orders.customer_id holds customers keys, payments.id holds payments keys"]),
    ("duckdb", "WITH RECURSIVE t(n) AS (VALUES (1) UNION ALL SELECT n + 1 FROM t WHERE n < 3) SELECT n FROM t", &["[unsupported-syntax] a VALUES list is not typed"]),
    ("postgres", "WITH RECURSIVE t(n) AS ((SELECT 1 UNION SELECT 2) UNION ALL SELECT n + 1 FROM t WHERE n < 3) SELECT n FROM t", &[]),
    ("postgres", "WITH RECURSIVE t(n) AS (VALUES (1) UNION ALL SELECT n + 1 FROM t WHERE n < 3) SELECT n FROM t", &[]),
    ("duckdb", "SELECT 1 AS n UNION ALL VALUES (2)", &["[unsupported-syntax] a VALUES list is not typed"]),
    ("postgres", "SELECT 1 AS n UNION ALL (SELECT 2)", &[]),
    ("postgres", "SELECT id FROM orders UNION TABLE orders", &[]),
    ("postgres", "SELECT o.note FROM orders o ORDER BY note, nope", &["[unknown-column] column `nope` does not exist in `o`; did you mean `note`?"]),
    ("postgres", "SELECT note AS n FROM orders ORDER BY n, 1", &[]),
    ("postgres", "SELECT unknown_fn(1).* FROM orders", &["[unknown-function] function `unknown_fn` is not known to sqlty, so its result is untyped"]),
    ("mysql", "UPDATE orders o JOIN customers c ON c.id = o.customer_id SET o.note = c.name, nope = 1", &["[unknown-column] column `nope` does not exist in `o`, `c`; did you mean `note`?"]),
    ("mysql", "DELETE o, c FROM orders o JOIN customers c ON c.id = o.customer_id WHERE c.nope = 1", &["[unknown-column] column `nope` does not exist in `c`"]),
    ("mysql", "INSERT INTO tags (order_id, label) VALUES (1, 'a') ON DUPLICATE KEY UPDATE label = VALUES(label)", &[]),
    ("mysql", "INSERT INTO tags SET order_id = 1, nope = 2", &["[unknown-column] column `nope` does not exist in `tags`"]),
    ("mysql", "INSERT INTO tags VALUES (1)", &["[insert-value-count] INSERT into `tags` gives 1 value for 2 columns"]),
    ("sqlite", "INSERT INTO tags VALUES (1, 'a'), (2)", &["[insert-value-count] INSERT into `tags` gives 1 value for 2 columns"]),
    ("postgres", "INSERT INTO tags (nope) VALUES (1)", &["[unknown-column] column `nope` does not exist in `tags`"]),
    ("postgres", "INSERT INTO orders AS o (id, customer_id) VALUES (1, 2) ON CONFLICT (id) DO UPDATE SET note = EXCLUDED.note WHERE o.nope = 1 RETURNING o.id", &["[unknown-column] column `nope` does not exist in `o`; did you mean `note`?"]),
    ("postgres", "INSERT INTO orders (id, customer_id, note) VALUES (1, 2, 'x') ON CONFLICT DO NOTHING RETURNING note", &[]),
    ("postgres", "INSERT INTO orders (id, customer_id) SELECT 1, 2 RETURNING *", &[]),
    ("postgres", "WITH v AS (SELECT 1) INSERT INTO tags (order_id) VALUES (1) RETURNING *", &[]),
    ("postgres", "MERGE INTO orders o USING customers c ON c.id = o.customer_id WHEN MATCHED THEN UPDATE SET note = c.name", &[]),
    ("postgres", "UPDATE orders SET note = 'x' WHERE note IS NOT NULL RETURNING note", &[]),
    ("postgres", "DELETE FROM orders WHERE note IS NOT NULL RETURNING note", &[]),
    ("postgres", "SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id AND c.id = o.id", &["[entity-mismatch] customers.id holds customers keys, orders.id holds orders keys"]),
    ("postgres", "SELECT * FROM orders o, customers c WHERE c.id = o.id", &["[entity-mismatch] customers.id holds customers keys, orders.id holds orders keys"]),
    ("postgres", "SELECT * FROM orders o JOIN payments p ON (p.id = o.id)", &["[entity-mismatch] payments.id holds payments keys, orders.id holds orders keys"]),
    ("postgres", "SELECT * FROM order_items i JOIN payments p USING (order_id)", &[]),
    ("postgres", "SELECT * FROM orders o JOIN payments p ON p.order_id = o.id WHERE p.id IN (SELECT o.customer_id FROM orders)", &["[foreign-key-mismatch] orders.customer_id references customers.id, not payments.id", "[entity-mismatch] payments.id holds payments keys, orders.customer_id holds customers keys"]),
    ("postgres", "SELECT o.id, MAX(p.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p ON p.order_id = o.id GROUP BY o.id, p.id", &[]),
    ("postgres", "SELECT o.id, MAX(p.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p ON p.order_id = o.id GROUP BY o.id, p.order_id", &[]),
    ("postgres", "SELECT o.id, SUM(i.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p ON p.order_id = o.id AND p.amount > 0 AND p.id = 5 GROUP BY o.id", &[]),
    ("postgres", "SELECT o.id, SUM(i.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p ON p.order_id = o.id AND 5 = p.id GROUP BY o.id", &[]),
    ("postgres", "SELECT o.id, SUM(i.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p ON p.order_id = o.id AND p.id = (SELECT MAX(x.id) FROM payments x WHERE x.order_id = o.id) GROUP BY o.id", &[]),
    ("postgres", "SELECT o.id, SUM(i.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p ON p.order_id = o.id AND p.id = (SELECT MAX(x.id) FROM payments x WHERE x.order_id = p.order_id) GROUP BY o.id", &["[fanout-aggregate] SUM(i.amount) aggregates order_items, whose rows repeat once per payments row", "[chasm-join] orders joins order_items and payments one-to-many, so each row of one repeats once per row of the other"]),
    ("postgres", "SELECT o.id, SUM(i.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p ON p.order_id = o.id AND p.id = abs(-1) GROUP BY o.id", &["[fanout-aggregate] SUM(i.amount) aggregates order_items, whose rows repeat once per payments row", "[chasm-join] orders joins order_items and payments one-to-many, so each row of one repeats once per row of the other"]),
    ("postgres", "SELECT o.id, SUM(i.amount + (SELECT 1)), SUM((p.amount)) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p ON p.order_id = o.id GROUP BY o.id", &["[fanout-aggregate] SUM(i.amount + (SELECT 1)) aggregates order_items, whose rows repeat once per payments row", "[fanout-aggregate] SUM((p.amount)) aggregates payments, whose rows repeat once per order_items row", "[chasm-join] orders joins order_items and payments one-to-many, so each row of one repeats once per row of the other"]),
    ("postgres", "SELECT o.id, SUM(i.amount) FROM orders o JOIN order_items i ON (i.order_id) = o.id JOIN (SELECT order_id FROM payments) USING (order_id) GROUP BY o.id", &[]),
    ("postgres", "SELECT o.id, SUM(i.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p USING (amount) GROUP BY o.id", &["[unproven-join] the join of order_items and payments matches no primary or unique key, so how often it repeats their rows is unknown"]),
    ("postgres", "SELECT o.id, SUM(d.t) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN (SELECT order_id, SUM(amount) t FROM payments GROUP BY ALL) d ON d.order_id = o.id GROUP BY o.id", &["[fanout-aggregate] SUM(d.t) aggregates d, whose rows repeat once per order_items row"]),
    ("postgres", "SELECT o.id, SUM(d.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN (SELECT p.order_id, p.amount FROM payments p JOIN orders x ON x.id = p.order_id) d ON d.order_id = o.id GROUP BY o.id", &["[fanout-aggregate] SUM(d.amount) aggregates d, whose rows repeat once per order_items row"]),
    ("postgres", "SELECT o.id, SUM(d.t) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN (SELECT p.order_id, SUM(p.amount) AS t FROM payments p GROUP BY order_id) d ON d.order_id = o.id GROUP BY o.id", &["[fanout-aggregate] SUM(d.t) aggregates d, whose rows repeat once per order_items row"]),
    ("postgres", "SELECT o.id, SUM(d.t) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN (SELECT order_id, SUM(amount) AS t FROM payments GROUP BY 9) d ON d.order_id = o.id GROUP BY o.id", &["[ungrouped-column] column `payments.order_id` must appear in GROUP BY or be used in an aggregate function", "[fanout-aggregate] SUM(d.t) aggregates d, whose rows repeat once per order_items row"]),
    ("postgres", "SELECT o.id, SUM(d.t) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN (SELECT order_id, SUM(amount) AS t FROM payments GROUP BY $1) d ON d.order_id = o.id GROUP BY o.id", &["[ungrouped-column] column `payments.order_id` must appear in GROUP BY or be used in an aggregate function", "[fanout-aggregate] SUM(d.t) aggregates d, whose rows repeat once per order_items row"]),
    ("postgres", "SELECT o.id, SUM(d.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN (SELECT DISTINCT * FROM payments) d ON d.order_id = o.id GROUP BY o.id", &["[fanout-aggregate] SUM(d.amount) aggregates d, whose rows repeat once per order_items row"]),
    ("postgres", "SELECT o.id, SUM(d.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN (SELECT amount FROM unnest(ARRAY[1]) AS amount) d ON true GROUP BY o.id", &[]),
    ("postgres", "SELECT o.id, SUM(d.n) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN (SELECT COUNT(*) AS n FROM payments) d ON true GROUP BY o.id", &["[fanout-aggregate] SUM(d.n) aggregates d, whose rows repeat once per orders row"]),
    ("postgres", "SELECT o.id, SUM(d.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN (WITH x AS (SELECT 1) SELECT id, amount FROM payments) d ON d.id = o.id GROUP BY o.id", &["[fanout-aggregate] SUM(d.amount) aggregates d, whose rows repeat once per order_items row"]),
    ("postgres", "SELECT o.id, SUM(d.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN (SELECT id, amount FROM payments UNION SELECT 1, 2) d ON d.id = o.id GROUP BY o.id", &["[fanout-aggregate] SUM(d.amount) aggregates d, whose rows repeat once per order_items row"]),
    ("postgres", "WITH c AS (SELECT DISTINCT ON (order_id) order_id, amount FROM payments) SELECT o.id, SUM(c.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN c ON c.order_id = o.id GROUP BY o.id", &["[fanout-aggregate] SUM(c.amount) aggregates c, whose rows repeat once per order_items row"]),
    ("postgres", "SELECT o.id, SUM(o2.customer_id) FROM orders o JOIN orders o2 ON o2.customer_id = o.customer_id JOIN order_items i ON i.order_id = o.id GROUP BY o.id", &["[fanout-aggregate] SUM(o2.customer_id) aggregates o2, whose rows repeat once per order_items row", "[unproven-join] the join of o and o2 matches no primary or unique key, so how often it repeats their rows is unknown"]),
    ("postgres", "SELECT o.id, AVG(p.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p ON p.order_id = o.id GROUP BY o.id, p.id", &["[chasm-join] orders joins order_items and payments one-to-many, so each row of one repeats once per row of the other"]),
    ("postgres", "SELECT o.id, AVG(p.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p ON p.order_id = o.id GROUP BY o.id, i.id", &["[fanout-aggregate] AVG(p.amount) aggregates payments, whose rows repeat once per order_items row", "[chasm-join] orders joins order_items and payments one-to-many, so each row of one repeats once per row of the other"]),
    ("postgres", "SELECT o.id, AVG(o.customer_id) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p ON p.id = i.id GROUP BY i.order_id", &["[ungrouped-column] column `o.id` must appear in GROUP BY or be used in an aggregate function", "[entity-mismatch] payments.id holds payments keys, order_items.id holds order_items keys"]),
    ("postgres", "SELECT o.id, SUM(i.amount + nope) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p ON p.order_id = o.id GROUP BY o.id", &["[fanout-aggregate] SUM(i.amount + nope) aggregates order_items, whose rows repeat once per payments row", "[chasm-join] orders joins order_items and payments one-to-many, so each row of one repeats once per row of the other", "[unknown-column] column `nope` does not exist in `o`, `i`, `p`; did you mean `note`?"]),
    ("postgres", "SELECT o.id, SUM(i.amount) FROM orders o JOIN order_items i ON i.order_id = o.id JOIN payments p USING (nope) GROUP BY o.id", &[]),
    ("mysql", "TABLE orders UNION TABLE orders", &[]),
    ("sqlite", "VALUES (1) UNION ALL VALUES (2)", &[]),
    ("postgres", "SELECT \"MyFn\"(1)", &["[unknown-function] function `\"MyFn\"` is not known to sqlty, so its result is untyped"]),
    ("mysql", "SELECT MyFn(1)", &["[unknown-function] function `myfn` is not known to sqlty, so its result is untyped"]),
    ("postgres", "SELECT * FROM lost", &["[unsupported-syntax] `*` reads view `lost`, whose columns are not typed"]),
    ("postgres", "SELECT 1 FROM orders o WHERE (o).* IS NOT NULL", &[]),
    ("postgres", "SELECT CASE WHEN true THEN true ELSE 1 END", &["[rejected-query] CASE types BOOLEAN and INT cannot be matched"]),
    ("postgres", "SELECT CASE WHEN true THEN now()::date ELSE 1 END", &["[rejected-query] CASE types DATE and INT cannot be matched"]),
    ("postgres", "SELECT CASE WHEN true THEN INTERVAL '1 day' ELSE 1 END", &["[rejected-query] CASE types INTERVAL and INT cannot be matched"]),
    ("postgres", "SELECT CASE WHEN true THEN '{}'::json ELSE 1 END", &["[rejected-query] CASE types JSON and INT cannot be matched"]),
    ("postgres", "SELECT CASE WHEN true THEN gen_random_uuid() ELSE 1 END", &["[rejected-query] CASE types UUID and INT cannot be matched"]),
    ("postgres", "SELECT CASE WHEN true THEN '1.2.3.4'::inet ELSE 1 END", &[]),
    ("postgres", "SELECT CASE WHEN true THEN 1 = 1 ELSE 'a' END", &[]),
    ("mysql", "SELECT \"nope\", @x, @@version, NoTe FROM orders", &[]),
    ("sqlite", "SELECT \"nope\" FROM orders", &[]),
    ("postgres", "SELECT 1 FROM orders o WHERE o IS NOT NULL", &[]),
    ("postgres", "INSERT INTO orders VALUES (DEFAULT, 1, 'x')", &[]),
    ("postgres", "SELECT o.id FROM orders o, customers o", &["[duplicate-alias] table name `o` is specified more than once"]),
    ("postgres", "UPDATE orders SET note = 'x' RETURNING old.note, new.note", &[]),
    ("postgres", "SELECT * FROM (SELECT * FROM nowhere) x", &["[unknown-table] table `nowhere` is not defined in the schema"]),
    ("postgres", "INSERT INTO orders (id, customer_id) VALUES (1, 2) ON CONFLICT (id) DO UPDATE SET (note, customer_id) = ('a', 3) RETURNING *", &[]),
    ("postgres", "INSERT INTO orders (id, customer_id) VALUES (1, 2) ON CONFLICT DO NOTHING RETURNING *", &[]),
    ("postgres", "INSERT INTO nope VALUES (1)", &["[unknown-table] table `nope` is not defined in the schema"]),
    ("postgres", "INSERT INTO tags SELECT * FROM nowhere", &["[unknown-table] table `nowhere` is not defined in the schema"]),
    ("clickhouse", "INSERT INTO FUNCTION remote('h', 'db.t') VALUES (1)", &[]),
    ("postgres", "UPDATE (SELECT 1) SET x = 1", &[]),
    ("postgres", "SELECT * FROM pg_class c", &[]),
    ("postgres", "SELECT * FROM (orders o JOIN customers c ON c.id = o.customer_id) AS j", &["[unsupported-syntax] `*` reads aliased join, which sqlty does not read the columns of"]),
    ("postgres", "SELECT * FROM generate_series(1, 2), unnest(ARRAY[1]), xmltable('/a' PASSING '<a/>' COLUMNS x INT)", &[]),
    ("postgres", "SELECT * FROM LATERAL (SELECT * FROM nowhere) AS x", &["[unknown-table] table `nowhere` is not defined in the schema"]),
    ("mysql", "SELECT * FROM JSON_TABLE('[]', '$[*]' COLUMNS (n FOR ORDINALITY, NESTED PATH '$.a' COLUMNS (x INT PATH '$'))) AS j", &["[unsupported-syntax] `*` reads table expression `JSON_TABLE('[]', '$[*]' COLUMNS(n FOR ORDINALITY, NESTED PATH '$.a' COLUMNS (x INT PATH '$'))) AS j`, which sqlty does not read the columns of"]),
    ("postgres", "SELECT * FROM json_array_elements('[1]'), json_each('{}'), jsonb_populate_record(NULL, '{}')", &["[unsupported-syntax] `*` reads table function jsonb_populate_record, which sqlty does not read the columns of"]),
    ("postgres", "SELECT * FROM order_count(), order_count() AS c", &[]),
    ("postgres", "SELECT 1 FROM orders o WHERE o.customer_id IN (SELECT id FROM payments UNION SELECT 1)", &[]),
    ("postgres", "SELECT 1 FROM orders o WHERE o.customer_id IN (SELECT o2.id FROM orders o2)", &["[foreign-key-mismatch] orders.customer_id references customers.id, not orders.id", "[entity-mismatch] orders.customer_id holds customers keys, orders.id holds orders keys"]),
    ("postgres", "SELECT g.*, COUNT(*) FROM generate_series(1, 2) g GROUP BY 1", &["[ungrouped-column] column `g.g` must appear in GROUP BY or be used in an aggregate function"]),
    ("postgres", "WITH c AS (SELECT id FROM orders) SELECT c.*, COUNT(*) FROM c GROUP BY id", &[]),
    ("postgres", "SELECT row_to_json(o) FROM orders o GROUP BY o.id", &[]),
    ("postgres", "SELECT row_to_json(o) FROM orders o GROUP BY o.note", &["[ungrouped-column] column `o.*` must appear in GROUP BY or be used in an aggregate function"]),
    ("postgres", "SELECT o.id, (SELECT o.note FROM customers c LIMIT 1) FROM orders o GROUP BY o.id", &[]),
    ("bigquery", "SELECT STRUCT(1 AS a).*, COUNT(*) FROM orders GROUP BY id", &["[unsupported-syntax] `<expression>.*` is not expanded"]),
    ("generic", "TABLE orders UNION TABLE orders", &[]),
    ("generic", "SELECT 1 UNION VALUES (2)", &["[unsupported-syntax] a VALUES list is not typed"]),
    ("postgres", "SELECT * FROM orders o JOIN pairs q ON o.id = q.a JOIN pairs p ON p.a = q.b", &["[foreign-key-mismatch] pairs.a references orders.id, not pairs.b", "[entity-mismatch] pairs.a holds orders keys, pairs.b holds customers keys"]),
    ("postgres", "SELECT 1 FROM orders o WHERE o.id IN (SELECT p.id FROM payments p) AND o.id IN (SELECT id, order_id FROM payments)", &["[entity-mismatch] orders.id holds orders keys, payments.id holds payments keys"]),
    ("databricks", "SELECT stack(1, 2, 3) AS (a, b) FROM orders GROUP BY id", &["[unknown-function] function `stack` is not known to sqlty, so its result is untyped"]),
    ("postgres", "SELECT *, COUNT(*) FROM unnest(ARRAY[1]) GROUP BY 1", &["[ungrouped-column] column `unnest.unnest` must appear in GROUP BY or be used in an aggregate function"]),
    ("postgres", "SELECT (SELECT c.name FROM customers c WHERE c.id = o.id GROUP BY c.id) FROM orders o", &["[entity-mismatch] customers.id holds customers keys, orders.id holds orders keys"]),
    ("postgres", "SELECT * FROM orders JOIN customers USING (nope)", &["[unsupported-syntax] `*` could not be expanded into columns"]),
    ("postgres", "SELECT o.nope FROM orders o, customers o", &["[duplicate-alias] table name `o` is specified more than once"]),
    ("mysql", "SELECT custumer_ID FROM orders", &["[unknown-column] column `custumer_ID` does not exist in `orders`; did you mean `customer_id`?"]),
    ("postgres", "SELECT z.* FROM orders o", &["[unknown-table] `z` in `z.*` is not a table or alias in the FROM clause"]),
    ("postgres", "SELECT * FROM information_schema.tables", &[]),
    ("postgres", "SELECT * FROM orders, LATERAL generate_series(1, id) g", &[]),
    ("generic", "SELECT * FROM (VALUES (1)) v", &["[unsupported-syntax] a VALUES list is not typed"]),
    ("postgres", "SELECT 0o9, 0x1_2", &[]),
    ("generic", "WITH RECURSIVE r AS (TABLE orders UNION ALL SELECT * FROM r) SELECT * FROM r", &["[unsupported-syntax] a TABLE query is not typed"]),
    ("duckdb", "SELECT id FROM orders ORDER BY ALL", &[]),
    ("postgres", "SELECT 1 UNION (WITH c AS (SELECT 2) SELECT * FROM c)", &[]),
    ("postgres", "SELECT l.* FROM lost l", &["[unsupported-syntax] `*` reads view `lost`, whose columns are not typed"]),
    ("generic", "SELECT 1 UNION TABLE orders", &["[unsupported-syntax] a TABLE query is not typed"]),
    ("sqlite", "SELECT * FROM generate_series(1, 3)", &["[unsupported-syntax] `*` reads table function generate_series, which sqlty does not read the columns of"]),
    ("duckdb", "SELECT * FROM range(3)", &["[unsupported-syntax] `*` reads table function range, which sqlty does not read the columns of"]),
    ("postgres", "SELECT all_orders()", &[]),
];

/// `(dialect, query, columns as "name: python (SQL)")`, against `SCHEMA`.
#[rustfmt::skip]
const COLUMNS: &[Case] = &[
    ("clickhouse", "SELECT CAST(1 AS Nullable(Int32)) AS a, CAST('x' AS LowCardinality(String)) AS b, CAST(tuple(1, 'a') AS Tuple(a Int32, b String)) AS t, CAST(map('a', 1) AS Map(String, Int32)) AS m, CAST([1] AS Array(Int32)) AS arr", &["a: int (INT)", "b: str (TEXT)", "t: tuple[int, str] (ROW(a INT, b TEXT))", "m: dict[str, int] (MAP(TEXT, INT))", "arr: list[int] (ARRAY<INT>)"]),
    ("mysql", "SELECT CAST(1 AS UNSIGNED) AS u, CAST('x' AS CHAR) COLLATE utf8mb4_bin AS c", &["u: int (UBIGINT)", "c: str (CHAR)"]),
    ("duckdb", "SELECT [1, 2]::INTEGER[2] AS fixed, []::INTEGER[] AS l, {'a': 1}::STRUCT(a INTEGER) AS s, MAP {'a': 1}::MAP(VARCHAR, INTEGER) AS m, 1::UNION(n INTEGER, s VARCHAR) AS u", &["fixed: tuple[int, ...] (ARRAY<INT>[2])", "l: list[int] (ARRAY<INT>)", "s: dict[str, int] (STRUCT(a INT))", "m: dict[str, int] (MAP(VARCHAR, INT))", "u: int | str (UNION(n INT, s VARCHAR))"]),
    ("duckdb", "SELECT 1::UINT8 AS a, 1::UINT16 AS b, 1::UINT32 AS c, 1::INT1 AS d, 1::FLOAT8 AS e, 1::DOUBLE PRECISION AS f, [[1]]::INTEGER[][] AS g", &["a: int (UTINYINT)", "b: int (USMALLINT)", "c: int (UINT)", "d: int (TINYINT)", "e: float (DOUBLE)", "f: float (DOUBLE)", "g: list[list[int]] (ARRAY<ARRAY<INT>>)"]),
    ("postgres", "SELECT ARRAY[]::INT[] AS a, ROW(1, 'a') AS r, NULL::BIT(3) AS b, INTERVAL '1 day' AS i, '1:00'::TIME WITHOUT TIME ZONE AS t, now()::TIMESTAMP WITHOUT TIME ZONE AS ts, 1::INT4 AS n", &["a: list[int] (ARRAY<INT>)", "r: tuple[typing.Any, ...] (RECORD)", "b: str | None (BIT(3))", "i: datetime.timedelta (INTERVAL)", "t: datetime.time (TIME)", "ts: datetime.datetime (TIMESTAMP)", "n: int (INT)"]),
    ("bigquery", "SELECT STRUCT(1 AS a, 'x' AS b) AS s, [1, 2] AS l, CAST(NULL AS ARRAY<STRUCT<a INT64>>) AS n", &["s: None (None)", "l: list[int] (ARRAY<INT>)", "n: list[dict[str, int]] | None (ARRAY<STRUCT(a BIGINT)>)"]),
    ("postgres", "SELECT '{\"a\": {\"b\": [1, 2]}, \"m\": null}'::jsonb AS j", &["j: typing.Any (JSONB) {\"object\":[{\"name\":\"a\",\"value\":{\"shape\":{\"object\":[{\"name\":\"b\",\"value\":{\"shape\":{\"array\":{\"shape\":\"integer\",\"nullable\":false}},\"nullable\":false},\"optional\":false}]},\"nullable\":false},\"optional\":false},{\"name\":\"m\",\"value\":{\"shape\":\"any\",\"nullable\":true},\"optional\":false}]}"]),
    ("postgres", "SELECT j -> 'a' -> 'b' -> 0 AS first, j -> 'a' -> 'b' -> -1 AS last, j #> '{a,b}' AS b, j #>> ARRAY['a', 'x'] AS x, j #> '{}' AS whole, j -> 'm' -> 'z' AS z, j -> ('a') AS a FROM (SELECT '{\"a\": {\"b\": [1, 2]}, \"m\": null}'::jsonb AS j) s", &["first: typing.Any (JSONB) \"integer\"", "last: typing.Any (JSONB) \"integer\"", "b: typing.Any (JSONB) {\"array\":{\"shape\":\"integer\",\"nullable\":false}}", "x: str | None (TEXT)", "whole: typing.Any (JSONB) {\"object\":[{\"name\":\"a\",\"value\":{\"shape\":{\"object\":[{\"name\":\"b\",\"value\":{\"shape\":{\"array\":{\"shape\":\"integer\",\"nullable\":false}},\"nullable\":false},\"optional\":false}]},\"nullable\":false},\"optional\":false},{\"name\":\"m\",\"value\":{\"shape\":\"any\",\"nullable\":true},\"optional\":false}]}", "z: typing.Any (JSONB)", "a: typing.Any (JSONB) {\"object\":[{\"name\":\"b\",\"value\":{\"shape\":{\"array\":{\"shape\":\"integer\",\"nullable\":false}},\"nullable\":false},\"optional\":false}]}"]),
    ("postgres", "SELECT jsonb_build_object('k', 1) -> 'k' AS k, jsonb_build_object('k', NULL) -> 'k' AS n, jsonb_build_array(1, 'a') -> (0) AS e, NULLIF('[1]'::jsonb, 'null') -> 0 AS f", &["k: typing.Any (JSONB) \"integer\"", "n: typing.Any (JSONB)", "e: typing.Any (JSONB)", "f: typing.Any (JSONB) \"integer\""]),
    ("postgres", "SELECT body -> 'x' AS x, meta -> 'y' AS y FROM docs WHERE body ? 'x' AND (meta IS NOT NULL OR meta -> 'y' IS NOT NULL)", &["x: typing.Any (JSONB)", "y: typing.Any (JSON)"]),
    ("postgres", "SELECT jsonb_object_agg(label, order_id) -> 'k' AS m, json_agg(label) -> 0 AS a FROM tags", &["m: typing.Any (JSONB) \"integer\"", "a: typing.Any (JSON) \"string\""]),
    ("mysql", "SELECT JSON_EXTRACT('{\"a\": true}', '$.a') AS a, JSON_OBJECT('k', TRUE) AS o, JSON_ARRAY(1, 2) AS l", &["a: typing.Any (JSON)", "o: typing.Any (JSON) {\"object\":[{\"name\":\"k\",\"value\":{\"shape\":\"boolean\",\"nullable\":false},\"optional\":false}]}", "l: typing.Any (JSON) {\"array\":{\"shape\":\"integer\",\"nullable\":false}}"]),
    ("sqlite", "SELECT json_extract('{\"a\": [1]}', '$.a[0]') AS a, '{\"a\": 1}' -> '$.a' AS b, json('[1]') ->> 0 AS c", &["a: typing.Any (None)", "b: str | None (TEXT)", "c: typing.Any (None)"]),
    ("postgres", "SELECT '[1, \"a\"]'::json -> 5 AS e, '\"s\"'::jsonb -> 'k' AS s, '{\"a\": 1}'::jsonb -> 'nope' AS n, CASE WHEN true THEN '{\"a\": 1}'::jsonb ELSE NULL END -> 'a' AS c", &["e: typing.Any (JSON)", "s: typing.Any (JSONB)", "n: typing.Any (JSONB)", "c: typing.Any (JSONB) \"integer\""]),
    ("postgres", "SELECT COALESCE(1, 'a') AS a, COALESCE((NULL), note) AS b, COALESCE(NULL, ?) AS c FROM orders", &["a: int (None)", "b: str | None (TEXT)", "c: None (None)"]),
    ("mysql", "SELECT STR_TO_DATE('x', note) AS a, STR_TO_DATE('x', NULL) AS b, CEIL(CAST(1 AS DECIMAL(30, 2))) AS c, CEIL(CAST(1 AS DECIMAL(30))) AS d FROM orders", &["a: None (None)", "b: None (None)", "c: decimal.Decimal (DECIMAL(30, 2))", "d: decimal.Decimal (DECIMAL(30))"]),
    ("duckdb", "SELECT date_part(note, now()) AS a, date_part(NULL, now()) AS b FROM orders", &["a: None (None)", "b: None (None)"]),
    ("postgres", "SELECT range_merge(1) AS a, lower(NULL) AS b, lower(?) AS c, AVG(INTERVAL '1 day') AS d, AVG(NULL) AS e, sqrt(note) AS f, timezone('UTC', 1) AS g, date_trunc('day', note) AS h, substr(B'1010', 1, 2) AS i, random(note, note) AS j FROM orders", &["a: None (None)", "b: None (None)", "c: None (None)", "d: datetime.timedelta | None (INTERVAL)", "e: None (None)", "f: None (None)", "g: None (None)", "h: None (None)", "i: str (BIT)", "j: None (None)"]),
    ("postgres", "SELECT SUM(t.doc) AS a FROM order_totals t", &["a: decimal.Decimal | None (NUMERIC)"]),
    ("postgres", "SELECT (note).x AS a, public.lower('a') AS b, \"Lower\"('a') AS c, 1e3 AS d, 99999999999999999999 AS e, CASE WHEN true THEN 'a' ELSE ? END AS f, percentile_disc(ARRAY[0.5]) WITHIN GROUP (ORDER BY NULL) AS g, note::ENUM('X', 'Y') AS h FROM orders ORDER BY id", &["a: None (None)", "b: None (None)", "c: None (None)", "d: decimal.Decimal (None)", "e: decimal.Decimal (None)", "f: typing.Literal['a'] (None)", "g: None (None)", "h: str | None (ENUM('X', 'Y'))"]),
    ("mysql", "SELECT db.f(1) AS a, 1e3 AS b, 99999999999999999999 AS c, 0xFFFFFFFFFFFFFFFFFFFF AS d, CAST(note AS ENUM('X', 'Y')) AS e FROM orders", &["a: None (None)", "b: float (None)", "c: int (None)", "d: bytes (VARBINARY)", "e: str | None (ENUM('X', 'Y'))"]),
    ("clickhouse", "SELECT 1.5 AS a", &["a: float (None)"]),
    ("duckdb", "SELECT NVL(note, 'x') AS a, NVL(note, note) AS b FROM orders", &["a: None (None)", "b: None (None)"]),
    ("duckdb", "SELECT {'a': 1}.*", &[]),
    ("bigquery", "SELECT STRUCT(1 AS a).*", &["*: None (None)"]),
    ("postgres", "WITH RECURSIVE r(n) AS ((SELECT 1) UNION ALL SELECT n + 1 FROM r) SELECT * FROM r", &["n: int (INT)"]),
    ("postgres", "WITH RECURSIVE r AS (TABLE orders UNION ALL SELECT * FROM r) SELECT * FROM r", &["id: int (INT)", "customer_id: int (INT)", "note: str | None (TEXT)"]),
    ("postgres", "(SELECT 1 AS a) UNION SELECT 2", &["a: int (None)"]),
    ("postgres", "SELECT SUM(id) OVER w AS a FROM orders WINDOW w AS (ORDER BY id)", &["a: int | None (BIGINT)"]),
    ("postgres", "SELECT id + 1 AS a, COUNT(*) AS b FROM orders GROUP BY ROLLUP (id + 1)", &["a: int | None (INT)", "b: int (BIGINT)"]),
    ("postgres", "SELECT u AS a FROM unnest((ARRAY[1, 2])) AS u", &["a: int | None (INT)"]),
    ("postgres", "SELECT u AS a FROM unnest((SELECT ARRAY[1])) AS u", &["a: int | None (INT)"]),
    ("postgres", "SELECT p.c AS a FROM orders o LEFT JOIN (SELECT COUNT(*) AS c FROM payments) p ON (TRUE)", &["a: int (BIGINT)"]),
    ("postgres", "SELECT (SELECT c.name FROM customers c WHERE (c.id = (o.customer_id))) AS a, (SELECT c.name FROM customers c WHERE c.id = o.customer_id AND c.name = 'x') AS b, (SELECT c.name FROM customers c WHERE c.id > o.customer_id) AS c, (SELECT c.name FROM customers c JOIN orders x ON x.id = c.id WHERE c.id = o.customer_id) AS d, (SELECT c.name FROM customers c WHERE c.id = o.id) AS e, (SELECT c.name FROM customers c WHERE o.id = o.customer_id) AS f FROM orders o", &["a: str | None (TEXT)", "b: str | None (TEXT)", "c: str | None (TEXT)", "d: str | None (TEXT)", "e: str | None (TEXT)", "f: str | None (TEXT)"]),
    ("postgres", "SELECT c.name AS a FROM orders o LEFT JOIN customers c ON (o.customer_id = c.id AND (c.id = o.customer_id))", &["a: str | None (TEXT)"]),
    ("postgres", "SELECT c.name AS a FROM orders o LEFT JOIN customers c ON o.customer_id = o.id", &["a: str | None (TEXT)"]),
    ("postgres", "SELECT x.id AS a FROM orders o LEFT JOIN LATERAL (SELECT id FROM orders LIMIT (1)) x ON true", &["a: int | None (INT)"]),
    ("postgres", "SELECT j #> ARRAY['a', 'b'] AS a, j #> ('{a}') AS b, j -> 'a'::text AS c, j -> t AS d, j #> '{a,b,0}' AS e, j -> 'a' -> 'b' -> 'c' AS f, jsonb_build_object('k') AS g, jsonb_build_object('k', j -> 'm', 'n', (NULL)) AS h, to_jsonb(s) AS i FROM (SELECT '{\"a\": {\"b\": [1, 2]}, \"m\": null}'::jsonb AS j, 1 AS t) s", &["a: typing.Any (JSONB) {\"array\":{\"shape\":\"integer\",\"nullable\":false}}", "b: typing.Any (JSONB) {\"object\":[{\"name\":\"b\",\"value\":{\"shape\":{\"array\":{\"shape\":\"integer\",\"nullable\":false}},\"nullable\":false},\"optional\":false}]}", "c: typing.Any (JSONB) {\"object\":[{\"name\":\"b\",\"value\":{\"shape\":{\"array\":{\"shape\":\"integer\",\"nullable\":false}},\"nullable\":false},\"optional\":false}]}", "d: typing.Any (JSONB)", "e: typing.Any (JSONB) \"integer\"", "f: typing.Any (JSONB)", "g: typing.Any (JSONB)", "h: typing.Any (JSONB) {\"object\":[{\"name\":\"k\",\"value\":{\"shape\":\"any\",\"nullable\":true},\"optional\":false},{\"name\":\"n\",\"value\":{\"shape\":\"any\",\"nullable\":true},\"optional\":false}]}", "i: typing.Any (JSONB) {\"object\":[{\"name\":\"j\",\"value\":{\"shape\":{\"object\":[{\"name\":\"a\",\"value\":{\"shape\":{\"object\":[{\"name\":\"b\",\"value\":{\"shape\":{\"array\":{\"shape\":\"integer\",\"nullable\":false}},\"nullable\":false},\"optional\":false}]},\"nullable\":false},\"optional\":false},{\"name\":\"m\",\"value\":{\"shape\":\"any\",\"nullable\":true},\"optional\":false}]},\"nullable\":false},\"optional\":false},{\"name\":\"t\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false}]}"]),
    ("postgres", "SELECT jsonb_build_object('k', note) AS a FROM orders note", &["a: typing.Any (JSONB) {\"object\":[{\"name\":\"k\",\"value\":{\"shape\":\"string\",\"nullable\":true},\"optional\":false}]}"]),
    ("postgres", "SELECT to_jsonb(o) AS a, to_jsonb(c) AS b FROM orders o LEFT JOIN customers c ON c.id = o.customer_id WHERE c IS NOT NULL AND (o IS NOT NULL) AND c.* IS NOT NULL AND (1 + 1) IS NOT NULL", &["a: typing.Any (JSONB) {\"object\":[{\"name\":\"id\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"customer_id\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"note\",\"value\":{\"shape\":\"string\",\"nullable\":true},\"optional\":false}]}", "b: typing.Any (JSONB) {\"object\":[{\"name\":\"id\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"email\",\"value\":{\"shape\":\"string\",\"nullable\":true},\"optional\":false},{\"name\":\"name\",\"value\":{\"shape\":\"string\",\"nullable\":false},\"optional\":false}]}"]),
    ("mysql", "SELECT JSON_OBJECT('k', 1 = 1, 'b', TRUE) AS a", &["a: typing.Any (JSON) {\"object\":[{\"name\":\"k\",\"value\":{\"shape\":\"any\",\"nullable\":false},\"optional\":false},{\"name\":\"b\",\"value\":{\"shape\":\"boolean\",\"nullable\":false},\"optional\":false}]}"]),
    ("postgres", "SELECT note::TRIGGER AS a FROM orders", &["a: None (TRIGGER)"]),
    ("postgres", "SELECT '{1}'::INT[] AS d", &["d: list[int] (ARRAY<INT>)"]),
    ("mysql", "SELECT CAST(1 AS INT ZEROFILL) AS a, CAST(1 AS INTEGER(11)) AS b", &["a: int (INT)", "b: int (INT(11))"]),
    ("clickhouse", "SELECT CAST(1 AS Nested(a Int32)) AS a", &["a: None (NESTED(A INT32))"]),
    ("mysql", "SELECT id + INTERVAL 1 DAY AS a, ADDDATE(1, 2) AS b, TIMESTAMPADD(DAY, 1, 5) AS c, id <=> 1 AS d, CASE WHEN true THEN TRUE ELSE 1 END AS e, IF(true, POINT(1, 2), 1) AS f, IF(true, CAST(1 AS UNSIGNED), CAST(2 AS UNSIGNED)) AS g, IF(true, 1, CAST(1 AS UNSIGNED)) AS h, IF(true, CAST(1 AS UNSIGNED), 1) AS i, IF(true, 1, 2) AS j FROM orders", &["a: None (None)", "b: None (None)", "c: None (None)", "d: bool (BOOLEAN)", "e: bool (None)", "f: bytes (POINT)", "g: int (UBIGINT)", "h: int (UBIGINT)", "i: int (UBIGINT)", "j: typing.Literal[1, 2] (None)"]),
    ("postgres", "SELECT -note AS a, note ~ 'a' AS b, @ id AS c FROM orders", &["a: str | None (TEXT)", "b: bool | None (BOOLEAN)", "c: int (INT)"]),
    ("sqlite", "SELECT CASE WHEN id THEN 1 ELSE 'a' END AS a, CASE WHEN id THEN 1 ELSE 1.5 END AS b FROM orders", &["a: int | str (None)", "b: float (DOUBLE)"]),
    ("snowflake", "SELECT CAST(NULL AS ARRAY) AS a", &["a: list | None (ARRAY)"]),
    ("postgres", "SELECT 0xFFFFFFFFFFFFFFFFFFFF AS a", &["a: decimal.Decimal (None)"]),
    ("sqlite", "SELECT 0xFFFFFFFFFFFFFFFFFFFF AS a", &["a: None (None)"]),
    ("mysql", "SELECT ADDDATE(CURRENT_DATE) AS a, CEIL(NULL) AS b, IF(true, 1 = 1, 2) AS c, IF(true, id, CAST(1 AS UNSIGNED)) AS d, IF(true, CAST(1 AS UNSIGNED), CAST(id AS UNSIGNED)) AS e, IF(true, id, CAST(1 AS SIGNED)) AS f FROM orders", &["a: None (None)", "b: None (None)", "c: int (INT)", "d: decimal.Decimal (DECIMAL)", "e: int (UBIGINT)", "f: int (BIGINT)"]),
    ("postgres", "SELECT SUBSTRING(B'1010' FROM 1 FOR 2) AS a, ~ note AS b, note @@ 'a' AS c, id / (2) AS d, id / '2' AS e, 1 = ANY((ARRAY[1])) AS f, 1 = ANY((SELECT ARRAY[id] FROM orders)) AS g FROM orders", &["a: str (BIT)", "b: str | None (TEXT)", "c: bool | None (BOOLEAN)", "d: int (INT)", "e: None (None)", "f: bool (BOOLEAN)", "g: bool (BOOLEAN)"]),
    ("sqlite", "SELECT CASE WHEN id THEN CASE WHEN id THEN 1 ELSE 'a' END ELSE 'b' END AS a FROM orders", &["a: int | str (None)"]),
    ("postgres", "SELECT (SELECT x.id FROM (SELECT id FROM customers) x WHERE x.id = o.customer_id) AS a, (SELECT c.name FROM customers c WHERE c.id = public.o.customer_id) AS b FROM orders o", &["a: int | None (INT)", "b: str | None (TEXT)"]),
    ("postgres", "SELECT CASE WHEN NULL IS DISTINCT FROM note THEN note END AS a, CASE WHEN (note = 'a') THEN 1 WHEN 'b' = note THEN 2 ELSE 3 END AS b FROM orders", &["a: str | None (TEXT)", "b: typing.Literal[1, 2, 3] (None)"]),
    ("postgres", "SELECT o.note AS a FROM orders o WHERE EXISTS (SELECT 1 UNION SELECT 2) AND EXISTS (SELECT 1 FROM customers c WHERE c.id = o.customer_id AND o.note = 'x')", &["a: str (TEXT)"]),
    ("postgres", "SELECT jsonb_build_array(1) -> 'k' AS a, to_jsonb(c) AS b FROM orders o LEFT JOIN customers c ON c.id = o.customer_id WHERE (c IS NOT NULL AND (o IS NOT NULL)) AND z.* IS NOT NULL", &["a: typing.Any (JSONB)", "b: typing.Any (JSONB) {\"object\":[{\"name\":\"id\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"email\",\"value\":{\"shape\":\"string\",\"nullable\":true},\"optional\":false},{\"name\":\"name\",\"value\":{\"shape\":\"string\",\"nullable\":false},\"optional\":false}]}"]),
    ("databricks", "SELECT stack(1, 2, 3) AS (a, b), 1 AS (c)", &["a: None (None)", "c: int (None)"]),
    ("mssql", "SELECT x.c FROM orders o OUTER APPLY (SELECT COUNT(*) AS c FROM payments) x", &["c: int (BIGINT)"]),
    ("postgres", "SELECT 1 AS a UNION (SELECT 2 ORDER BY 1)", &["a: int (None)"]),
    ("mysql", "SELECT CASE WHEN true THEN 'a' ELSE CURRENT_DATE END AS a, COALESCE('a', CURRENT_DATE) AS b, id / (2) AS c, id / 'x' AS d FROM orders", &["a: str (TEXT)", "b: str (TEXT)", "c: decimal.Decimal (DECIMAL)", "d: float | None (DOUBLE)"]),
    ("postgres", "SELECT TRIM(B'1010') AS a, OVERLAY(B'1010' PLACING B'1' FROM 1) AS b, - 'a'::date AS c, note ^@ 'a' AS d, id <-> 1 AS e, CASE WHEN note = 'a' AND note = 'b' THEN 1 END AS f FROM orders", &["a: str (BIT)", "b: str (BIT)", "c: datetime.date (DATE)", "d: bool | None (BOOLEAN)", "e: None (None)", "f: typing.Literal[1] | None (None)"]),
    ("postgres", "SELECT CASE WHEN (c IS NOT NULL AND o IS NOT NULL) THEN to_jsonb(c) END AS a, jsonb_agg(c) FILTER (WHERE (c IS NOT NULL AND z.* IS NOT NULL)) AS b, to_jsonb(x) AS d FROM orders o LEFT JOIN customers c ON c.id = o.customer_id, pg_class x GROUP BY o.id", &["a: typing.Any (JSONB) {\"object\":[{\"name\":\"id\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"email\",\"value\":{\"shape\":\"string\",\"nullable\":true},\"optional\":false},{\"name\":\"name\",\"value\":{\"shape\":\"string\",\"nullable\":false},\"optional\":false}]}", "b: typing.Any (JSONB) {\"array\":{\"shape\":{\"object\":[{\"name\":\"id\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"email\",\"value\":{\"shape\":\"string\",\"nullable\":true},\"optional\":false},{\"name\":\"name\",\"value\":{\"shape\":\"string\",\"nullable\":false},\"optional\":false}]},\"nullable\":false}}", "d: typing.Any (JSONB) {\"object\":[{\"name\":\"oid\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"relname\",\"value\":{\"shape\":\"string\",\"nullable\":false},\"optional\":false},{\"name\":\"relnamespace\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"reltype\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"reloftype\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"relowner\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"relam\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"relfilenode\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"reltablespace\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"relpages\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"reltuples\",\"value\":{\"shape\":\"number\",\"nullable\":false},\"optional\":false},{\"name\":\"relallvisible\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"reltoastrelid\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"relhasindex\",\"value\":{\"shape\":\"boolean\",\"nullable\":false},\"optional\":false},{\"name\":\"relisshared\",\"value\":{\"shape\":\"boolean\",\"nullable\":false},\"optional\":false},{\"name\":\"relpersistence\",\"value\":{\"shape\":\"string\",\"nullable\":false},\"optional\":false},{\"name\":\"relkind\",\"value\":{\"shape\":\"string\",\"nullable\":false},\"optional\":false},{\"name\":\"relnatts\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"relchecks\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"relhasrules\",\"value\":{\"shape\":\"boolean\",\"nullable\":false},\"optional\":false},{\"name\":\"relhastriggers\",\"value\":{\"shape\":\"boolean\",\"nullable\":false},\"optional\":false},{\"name\":\"relhassubclass\",\"value\":{\"shape\":\"boolean\",\"nullable\":false},\"optional\":false},{\"name\":\"relrowsecurity\",\"value\":{\"shape\":\"boolean\",\"nullable\":false},\"optional\":false},{\"name\":\"relforcerowsecurity\",\"value\":{\"shape\":\"boolean\",\"nullable\":false},\"optional\":false},{\"name\":\"relispopulated\",\"value\":{\"shape\":\"boolean\",\"nullable\":false},\"optional\":false},{\"name\":\"relreplident\",\"value\":{\"shape\":\"string\",\"nullable\":false},\"optional\":false},{\"name\":\"relispartition\",\"value\":{\"shape\":\"boolean\",\"nullable\":false},\"optional\":false},{\"name\":\"relrewrite\",\"value\":{\"shape\":\"integer\",\"nullable\":false},\"optional\":false},{\"name\":\"relfrozenxid\",\"value\":{\"shape\":\"string\",\"nullable\":false},\"optional\":false},{\"name\":\"relminmxid\",\"value\":{\"shape\":\"string\",\"nullable\":false},\"optional\":false},{\"name\":\"relacl\",\"value\":{\"shape\":{\"array\":{\"shape\":\"string\",\"nullable\":false}},\"nullable\":true},\"optional\":false},{\"name\":\"reloptions\",\"value\":{\"shape\":{\"array\":{\"shape\":\"string\",\"nullable\":false}},\"nullable\":true},\"optional\":false},{\"name\":\"relpartbound\",\"value\":{\"shape\":\"string\",\"nullable\":true},\"optional\":false}]}"]),
    ("postgres", "WITH s AS (SELECT CASE WHEN id = 1 THEN 'a' ELSE 'b' END AS k, note FROM orders) SELECT k FROM s WHERE note = 'x'", &["k: typing.Literal['a', 'b'] (None)"]),
    ("sqlite", "INSERT INTO orders (id, customer_id) VALUES (1, 2) ON CONFLICT DO NOTHING RETURNING *", &["id: int (INT)", "customer_id: int (INT)", "note: str | None (TEXT)"]),
    ("mysql", "INSERT INTO orders (id, customer_id) VALUES (1, 2) ON DUPLICATE KEY UPDATE note = 'x' RETURNING *", &["id: int (INT)", "customer_id: int (INT)", "note: str | None (TEXT)"]),
    ("mysql", "SELECT 1.5, N'x', \"dq\", 'sq'", &["1.5: decimal.Decimal (None)", "x: str (None)", "dq: str (None)", "sq: typing.Literal['sq'] (None)"]),
];

/// `(dialect, schema, columns of SELECT * FROM t, then each skipped object)`.
#[rustfmt::skip]
const SCHEMAS: &[Case] = &[
    ("sqlite", "CREATE VIRTUAL TABLE t USING fts5(title, body)", &["title: None ()", "body: None ()"]),
    ("sqlite", "CREATE VIRTUAL TABLE t USING nope(a)", &["*: None (None)", "skipped table t"]),
    ("sqlite", "CREATE VIRTUAL TABLE t USING rtree(id, x0, x1)", &["id: int (INT)", "x0: float (FLOAT)", "x1: float (FLOAT)"]),
    ("postgres", "CREATE TABLE p (id INT); CREATE TABLE t (LIKE p INCLUDING ALL, extra TEXT CHECK (length(extra) > 0) NOT VALID)", &["*: None (None)", "skipped table t"]),
    ("postgres", "CREATE TABLE t (id INT, owner INT REFERENCES users ON DELETE SET NULL (owner))", &["id: int | None (INT)", "owner: int | None (INT)"]),
    ("postgres", "CREATE TABLE t (id INT, at DATE) PARTITION BY RANGE (at) WITH (fillfactor = 70) TABLESPACE fast", &["id: int | None (INT)", "at: datetime.date | None (DATE)"]),
    ("mysql", "CREATE TABLE t (id INT UNSIGNED ZEROFILL, name TEXT COLLATE utf8mb4_bin) PARTITION BY HASH(id) PARTITIONS 4", &["id: int | None (UINT)", "name: str | None (TEXT)"]),
    ("mysql", "CREATE TABLE t AS SELECT 1 AS n", &["n: int (None)"]),
    ("postgres", "CREATE TABLE t (n SERIAL, s SMALLSERIAL, b BIGSERIAL, s2 SERIAL2)", &["n: int (INT)", "s: int (SMALLINT)", "b: int (BIGINT)", "s2: int (SMALLINT)"]),
    ("mysql", "CREATE TABLE t (n SERIAL)", &["n: int (UBIGINT)"]),
    ("postgres", "CREATE DOMAIN pos AS INT COLLATE \"C\" CHECK (VALUE > 0); CREATE TABLE t (n pos)", &["n: int | None (INT)"]),
    ("postgres", "CREATE TYPE mood AS ENUM ('sad', 'ok'); CREATE TYPE pair AS (a INT, b TEXT); CREATE TYPE r AS RANGE (subtype = int4); CREATE TABLE t (m mood, p pair, x r)", &["m: typing.Literal['sad', 'ok'] | None (MOOD)", "p: None (PAIR)", "x: None (R)"]),
    ("postgres", "CREATE TYPE broken AS NOPE; CREATE TABLE t (id INT)", &["id: int | None (INT)", "skipped statement CREATE TYPE broken AS NOPE"]),
    ("postgres", "CREATE TABLE u (id INT); CREATE VIEW t AS SELECT id FROM u WITH LOCAL CHECK OPTION", &["id: int | None (INT)"]),
    ("postgres", "CREATE VIEW t AS SELECT FROM WHERE", &["*: None (None)", "skipped view t"]),
    ("postgres", "CREATE TABLE u (id INT); CREATE FUNCTION f() RETURNS SETOF u LANGUAGE sql AS 'SELECT * FROM u'; CREATE FUNCTION g( RETURNS INT; CREATE VIEW t AS SELECT * FROM f()", &["id: int | None (INT)", "skipped function g"]),
    ("postgres", "CREATE TABLE t (id INT, email TEXT); CREATE UNIQUE INDEX t_email ON t (lower(email)); CREATE UNIQUE INDEX t_id ON t (id) WHERE id > 0", &["id: int | None (INT)", "email: str | None (TEXT)"]),
    ("mysql", "CREATE TABLE a (id INT); CREATE TABLE b (id INT); RENAME TABLE a TO t, b TO c", &["id: int | None (INT)"]),
    ("postgres", "CREATE TABLE t (id INT); ALTER TABLE t ALTER COLUMN id TYPE BIGINT USING id::bigint, ALTER COLUMN id SET DEFAULT 1, ALTER COLUMN id OPTIONS (ADD x 'y')", &["id: int | None (BIGINT)"]),
    ("postgres", "CREATE TABLE t (id INT); ALTER TABLE t ADD COLUMN IF NOT EXISTS x INT, RENAME COLUMN x TO y, DISABLE ROW LEVEL SECURITY", &["id: int | None (INT)", "y: int | None (INT)"]),
    ("postgres", "CREATE TABLE t (id INT); ALTER TABLE t SET NOPE", &["id: int | None (INT)"]),
    ("postgres", "CREATE TABLE t (id INT); ALTER TABLE t ADD PRIMARY KEY USING INDEX nope DEFERRABLE", &["id: int | None (INT)"]),
    ("postgres", "CREATE TYPE mood AS ENUM ('sad'); ALTER TYPE mood ADD VALUE 'ok'; ALTER TYPE mood RENAME VALUE 'sad' TO 'blue'; ALTER TYPE mood OWNER TO x; CREATE TABLE t (m mood)", &["m: typing.Literal['blue', 'ok'] | None (MOOD)"]),
    ("postgres", "CREATE TYPE mood AS ENUM ('sad'); ALTER TYPE mood SET SCHEMA other; CREATE TABLE t (m mood)", &["m: None (MOOD)", "skipped type mood"]),
    ("postgres", "CREATE DOMAIN pos AS INT; ALTER DOMAIN pos ADD CONSTRAINT p CHECK (VALUE > 0); CREATE TABLE t (n pos)", &["n: None (POS)", "skipped type pos"]),
    ("postgres", "CREATE TABLE u (id INT); CREATE VIEW v AS SELECT id FROM u; ALTER VIEW v RENAME TO t", &["*: None (None)", "skipped table v"]),
    ("postgres", "CREATE TABLE t (id INT); CREATE FUNCTION f() RETURNS INT LANGUAGE sql AS 'SELECT 1'; ALTER FUNCTION f() RENAME TO g", &["id: int | None (INT)"]),
    ("postgres", "CREATE TABLE t (id INT); CREATE TABLE u (id INT); DROP TABLE u CASCADE; DROP VIEW IF EXISTS nope; DROP MATERIALIZED VIEW m; DROP TYPE IF EXISTS x; DROP SEQUENCE s", &["id: int | None (INT)"]),
    ("mysql", "CREATE TABLE t (id INT, k INT, UNIQUE KEY uk (k), CONSTRAINT fk FOREIGN KEY (id) REFERENCES u (id)); ALTER TABLE t DROP INDEX uk, DROP FOREIGN KEY fk", &["id: int | None (INT)", "k: int | None (INT)"]),
    ("postgres", "CREATE TABLE t (id INT GENERATED ALWAYS AS (1) STORED, x INT GENERATED ALWAYS AS (t.id + 1) STORED)", &["id: int | None (INT)", "x: int | None (INT)"]),
    ("duckdb", "CREATE TABLE t (a INTEGER[3], b UNION(n INT, s TEXT), c STRUCT(x INT, y TEXT)[], d MAP(TEXT, INT), e INT[])", &["a: tuple[int, ...] | None (ARRAY<INT>[3])", "b: int | str | None (UNION(n INT, s TEXT))", "c: list[dict[str, int | str]] | None (ARRAY<STRUCT(x INT, y TEXT)>)", "d: dict[str, int] | None (MAP(TEXT, INT))", "e: list[int] | None (ARRAY<INT>)"]),
    ("bigquery", "CREATE TABLE t (a ARRAY<INT64>, b STRUCT<x INT64, y STRING>, c ARRAY<STRUCT<z BOOL>>)", &["a: list[int] | None (ARRAY<BIGINT>)", "b: dict[str, int | str] | None (STRUCT(x BIGINT, y TEXT))", "c: list[dict[str, bool]] | None (ARRAY<STRUCT(z BOOLEAN)>)"]),
    ("postgres", "CREATE TABLE t (a INT ARRAY, b int4[3], c TEXT COLLATE \"C\", d INTEGER)", &["a: list[int] | None (ARRAY<INT>)", "b: list[int] | None (ARRAY<INT>[3])", "c: str | None (TEXT)", "d: int | None (INT)"]),
    ("clickhouse", "CREATE TABLE t (a Array(Int32), b Map(String, Array(Nullable(Int64))), c Tuple(x Int32, String)) ENGINE = Memory", &["a: list[int] | None (ARRAY<INT>)", "b: dict[str, list[int]] | None (MAP(TEXT, ARRAY<BIGINT>))", "c: tuple[int, str] | None (ROW(x INT, TEXT))"]),
    ("postgres", "CREATE TABLE t (a STRUCT(x INT, y TEXT), b UNION(n INT, m TEXT), c MAP(TEXT, INT))", &["a: dict[str, int | str] | None (STRUCT(x INT, y TEXT))", "b: int | str | None (UNION(n INT, m TEXT))", "c: dict[str, int] | None (MAP(TEXT, INT))"]),
    ("postgres", "CREATE TABLE t (d ROW(x INT), e MAP(TEXT), h INT4(4))", &["d: tuple[int] | None (ROW(x INT))", "e: None (MAP(TEXT))", "h: int | None (INT(4))"]),
    ("postgres", "CREATE TABLE t (f STRUCT(x NOPE, y INT))", &["f: dict[str, typing.Any | int] | None (STRUCT(x NOPE, y INT))"]),
    ("sqlite", "CREATE TABLE t (a STRUCT(x INT), b INT4, c MAP(TEXT, INT, INT))", &["a: dict[str, int] | None (STRUCT(x INT))", "b: int | None (INT)", "c: float | None (MAP(TEXT, INT, INT))"]),
    ("duckdb", "CREATE TABLE u (a INTEGER[3], b INT, c STRUCT(x INT, y TEXT)); CREATE VIEW t AS SELECT a, b, c, CAST(NULL AS MAP(TEXT, INT)) AS d FROM u", &["a: tuple[int, ...] | None (ARRAY<INT>[3])", "b: int | None (INT)", "c: dict[str, int | str] | None (STRUCT(x INT, y TEXT))", "d: dict[str, int] | None (MAP(TEXT, INT))"]),
    ("postgres", "CREATE TABLE u (id INT PRIMARY KEY); CREATE TABLE t (id INT, x INT); ALTER TABLE nope ADD COLUMN x INT; ALTER TABLE t ADD CONSTRAINT f FOREIGN KEY (nope) REFERENCES u (id), RENAME COLUMN id TO \"Id\", ALTER COLUMN x ADD GENERATED ALWAYS AS IDENTITY", &["Id: int | None (INT)", "x: int | None (INT)"]),
    ("postgres", "CREATE TABLE p (id INT); CREATE TABLE c (x INT) INHERITS (p); CREATE TABLE t (y INT) INHERITS (c); ALTER TABLE p RENAME TO q; ALTER TABLE q ADD COLUMN z INT", &["id: int | None (INT)", "x: int | None (INT)", "y: int | None (INT)", "z: int | None (INT)"]),
    ("postgres", "CREATE TYPE mood AS ENUM ('sad'); ALTER TYPE mood RENAME VALUE 'nope' TO 'x'; ALTER TYPE mood RENAME TO m2; ALTER TYPE m2 RENAME TO m3; CREATE TYPE pair AS (a INT); ALTER TYPE pair ADD VALUE 'x'; ALTER TYPE nope SET SCHEMA x; ALTER FUNCTION nope(int) OWNER TO x; CREATE TABLE t (a mood, b m3, c pair)", &["a: typing.Literal['sad'] | None (M3)", "b: typing.Literal['sad'] | None (M3)", "c: None (PAIR)", "skipped type pair"]),
    ("mysql", "CREATE TABLE u (id INT); CREATE TABLE t (id INT, k INT, FOREIGN KEY (id) REFERENCES u (id)); ALTER TABLE t ADD COLUMN x INT AFTER nope; ALTER TABLE t CHANGE nope y INT; ALTER TABLE t DROP FOREIGN KEY x", &["id: int | None (INT)", "k: int | None (INT)"]),
    ("postgres", "CREATE TABLE t (id INT); ALTER TABLE t OF pair", &["*: None (None)", "skipped table t"]),
    ("postgres", "CREATE TABLE t (id INT); CREATE TRIGGER tr BEFORE INSERT ON nope FOR EACH ROW EXECUTE FUNCTION f()", &["id: int | None (INT)"]),
    ("mysql", "CREATE TABLE t (id INT NOT NULL); CREATE VIEW v AS SELECT 1 AS a; CREATE TRIGGER tr BEFORE INSERT ON t FOR EACH ROW SET NEW.id = 1; DROP TRIGGER tr", &["id: int (INT)"]),
    ("sqlite", "CREATE TABLE t (id INTEGER, x TEXT, PRIMARY KEY (id))", &["id: int (INT)", "x: str | None (TEXT)"]),
    ("sqlite", "CREATE VIEW t AS SELECT 1 AS n, 'a' AS s", &["n: int (None)", "s: typing.Literal['a'] (None)"]),
    ("postgres", "CREATE TABLE u (id INT); CREATE VIEW t AS WITH c AS (SELECT id FROM u) SELECT id FROM c", &["id: int | None (INT)"]),
    ("postgres", "CREATE TABLE t (id INT); CREATE UNIQUE INDEX i ON t (id); DROP INDEX i; CREATE TYPE x AS ENUM ('a'); DROP TYPE x; CREATE SCHEMA s; CREATE TABLE s.u (id INT); DROP SCHEMA s CASCADE; CREATE DOMAIN d AS INT; DROP DOMAIN d; CREATE FUNCTION f(a INT) RETURNS INT LANGUAGE sql AS 'SELECT 1'; DROP FUNCTION f(INT); DROP FUNCTION nope", &["id: int | None (INT)"]),
    ("postgres", "CREATE TABLE t ()", &["*: None (None)"]),
    ("postgres", "CREATE TABLE t (id INT); CREATE TABLE u (id INT); DROP TABLE u RESTRICT CASCADE weird; DROP MATERIALIZED VIEW IF EXISTS m WITH x; DROP TYPE x WEIRD; DROP DOMAIN d WEIRD", &["id: int | None (INT)", "skipped table u"]),
    ("sqlite", "CREATE VIRTUAL TABLE t USING rtree(id, x0 weird(, x1)", &["*: None (None)", "skipped table t USING rtree"]),
    ("sqlite", "CREATE VIRTUAL TABLE t USING rtree(id)", &["id: int (INT)"]),
    ("snowflake", "CREATE TABLE u (a ARRAY, b INT); CREATE VIEW t AS SELECT a, b FROM u", &["a: list | None (ARRAY)", "b: int | None (INT)"]),
    ("generic", "CREATE VIEW t AS VALUES (1)", &["*: None (None)"]),
    ("mysql", "CREATE TABLE u (a TINYINT UNSIGNED, b INT UNSIGNED); CREATE VIEW t AS SELECT IF(true, a, b) AS c FROM u", &["c: int | None (UINT)"]),
    ("mysql", "CREATE TABLE t2 ENGINE = InnoDB; CREATE VIEW v AS SELECT FROM WHERE; CREATE TABLE t (id INT); CREATE INDEX i ON t (id)", &["id: int | None (INT)", "skipped view v"]),
    ("postgres", "CREATE TABLE t (id INT); ALTER FUNCTION nope RENAME TO g; ALTER TABLE nope ADD CONSTRAINT c CHECK (true)", &["id: int | None (INT)", "skipped function nope"]),
    ("mysql", "CREATE TABLE t (id INT NOT NULL); CREATE TABLE z (id INT); CREATE TRIGGER tr BEFORE INSERT ON t FOR EACH ROW SET NEW.id = 1; DROP TRIGGER tr", &["id: int (INT)"]),
    ("postgres", "CREATE TABLE p (id INT); CREATE TABLE o (k INT); CREATE TABLE a () INHERITS (p); CREATE TABLE t () INHERITS (p, a); CREATE TABLE b () INHERITS (o); ALTER TABLE p RENAME TO q; ALTER TABLE q ADD COLUMN z INT", &["id: int | None (INT)", "z: int | None (INT)"]),
    ("postgres", "CREATE TABLE u (id INT PRIMARY KEY); CREATE TABLE t (id INT, x INT); CREATE VIEW v AS SELECT 1 AS n; CREATE UNIQUE INDEX i ON t (id); DROP INDEX i; ALTER TABLE t ADD CONSTRAINT f FOREIGN KEY (x) REFERENCES u (id), DROP COLUMN IF EXISTS nope, ALTER COLUMN nope SET DEFAULT 1", &["id: int | None (INT)", "x: int | None (INT)"]),
    ("postgres", "CREATE TYPE mood AS ENUM ('a'); CREATE TYPE other AS ENUM ('o'); ALTER TYPE mood RENAME TO m2; ALTER TYPE other RENAME TO o2; CREATE TYPE mood AS ENUM ('b'); CREATE TYPE pair AS (a INT); ALTER TYPE pair RENAME TO p2; CREATE TYPE pair AS (b TEXT); CREATE TABLE t (a mood, b m2, c pair, d p2)", &["a: typing.Literal['b'] | None (MOOD)", "b: typing.Literal['a'] | None (M2)", "c: None (PAIR)", "d: None (P2)"]),
    ("sqlite", "CREATE TABLE t (id INTEGER, x TEXT, PRIMARY KEY (id COLLATE nocase))", &["id: int | None (INT)", "x: str | None (TEXT)"]),
    ("sqlite", "CREATE VIRTUAL TABLE t USING fts5", &[]),
    ("duckdb", "CREATE TABLE t (a STRUCT(x INT, y INT))", &["a: dict[str, int] | None (STRUCT(x INT, y INT))"]),
    ("postgres", "CREATE TABLE c AS SELECT 1 AS n; ALTER TABLE c ADD CONSTRAINT k CHECK (n > 0); CREATE TABLE t (id INT); ALTER TABLE t ADD CONSTRAINT f FOREIGN KEY (id) REFERENCES nowhere (id)", &["id: int | None (INT)"]),
    ("postgres", "CREATE TABLE p (id INT); CREATE VIEW v AS SELECT 1 AS n; CREATE TABLE t () INHERITS (p); ALTER TABLE p RENAME TO q", &["id: int | None (INT)"]),
    ("postgres", "CREATE TYPE mood AS ENUM ('a'); ALTER TYPE mood RENAME TO m2; DROP TYPE m2; CREATE TABLE t (a mood)", &["a: None (MOOD)"]),
    ("sqlite", "CREATE TABLE u (a INT); CREATE TABLE t (id INTEGER, x TEXT, PRIMARY KEY (nope)); CREATE TABLE z (id INTEGER, x TEXT, PRIMARY KEY (id, x))", &["id: int | None (INT)", "x: str | None (TEXT)"]),
    ("postgres", "CREATE TABLE t AS SELECT 1 AS n, 'a' AS s", &["n: int | None (None)", "s: str | None (TEXT)"]),
    ("postgres", "CREATE TABLE u (id INT); CREATE VIEW t AS SELECT id FROM u; DROP TABLE u", &["id: int | None (INT)"]),
];

/// `(dialect, query, parameters as "index name: python (SQL) nullability")`, against `SCHEMA`.
#[rustfmt::skip]
const PARAMS: &[Case] = &[
    ("postgres", "UPDATE orders o SET note = %s FROM customers c WHERE c.id = o.customer_id AND c.name = %s", &["1: str (TEXT) Nullable", "2: str (TEXT) Unknown"]),
    ("mysql", "UPDATE orders, customers SET orders.note = ? WHERE customers.name = ?", &["1: str (TEXT) Nullable", "2: str (TEXT) Unknown"]),
    ("postgres", "SELECT %s UNION SELECT id FROM orders WHERE id = %s", &["1: None (None) Unknown", "2: int (INT) Unknown"]),
    ("postgres", "VALUES (%s::int, %s)", &["1: int (INT) Unknown cast", "2: None (None) Unknown"]),
    ("postgres", "INSERT INTO nope (a) VALUES (%s)", &["1: None (None) Unknown"]),
    ("postgres", "INSERT INTO orders (nope, note) VALUES (%s, %s)", &["1: None (None) Unknown", "2: str (TEXT) Nullable"]),
    ("postgres", "SELECT 1 FROM tags WHERE label LIKE ANY (%s) AND label LIKE ANY (ARRAY['a', %s]) AND label LIKE %s", &["1: list[str] (ARRAY<TEXT>) Unknown", "2: str (TEXT) Unknown", "3: str (TEXT) Unknown"]),
    ("postgres", "SELECT 1 FROM tags WHERE ARRAY[order_id] @> %s AND ARRAY[order_id] || %s IS NOT NULL AND ARRAY[order_id] && %s", &["1: list[int] (ARRAY<INT>) Unknown", "2: list[int] (ARRAY<INT>) Unknown", "3: list[int] (ARRAY<INT>) Unknown"]),
    ("postgres", "SELECT 1 FROM orders WHERE ANY(%s) = id AND %s = ANY(ARRAY[id]) AND id = ALL(ARRAY[%s])", &["1: list[int] (ARRAY<INT>) Unknown", "2: int (INT) Unknown", "3: int (INT) Unknown"]),
    ("postgres", "SELECT COALESCE(%s, %s, note), lower(%s), my_fn(%s) FROM orders", &["1: str (TEXT) Nullable", "2: str (TEXT) Nullable", "3: str (TEXT) Unknown", "4: None (None) Unknown"]),
    ("postgres", "SELECT now() + %s, %s - INTERVAL '1 day', INTERVAL '1 day' * %s, %s / 2 * INTERVAL '1 day', '1.2.3.4'::inet << %s, '1.2.3.4'::inet <<= %s", &["1: datetime.timedelta (INTERVAL) Unknown", "2: None (None) Unknown untyped INTERVAL", "3: float (DOUBLE) Unknown", "4: int (None) Unknown", "5: ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface (INET) Unknown", "6: ipaddress.IPv4Address | ipaddress.IPv6Address | ipaddress.IPv4Interface | ipaddress.IPv6Interface (INET) Unknown"]),
    ("postgres", "SELECT 1 FROM orders WHERE (id, note) = (%s, %s)", &["1: int (INT) Unknown", "2: str (TEXT) Unknown"]),
    ("postgres", "DELETE FROM orders WHERE id = %s", &["1: int (INT) Unknown"]),
    ("mysql", "DELETE FROM orders WHERE id = ? LIMIT ?", &["1: int (INT) Unknown", "2: int (BIGINT) Unknown"]),
    ("postgres", "WITH x AS (UPDATE orders SET note = %s RETURNING id) SELECT * FROM x", &["1: str (TEXT) Nullable"]),
    ("postgres", "SELECT $1::int + $1::text", &["1: int (INT) Unknown cast"]),
    ("mysql", "DELETE FROM orders LIMIT ?", &["1: int (BIGINT) Unknown"]),
    ("postgres", "SELECT %s UNION TABLE orders", &["1: None (None) Unknown"]),
    ("clickhouse", "INSERT INTO FUNCTION remote('h', 'db.t') VALUES (?)", &["1: None (None) Unknown"]),
    ("snowflake", "SELECT 1 FROM tags WHERE label LIKE ANY (?, ?) AND label LIKE ANY ('a', ?)", &["1: str (TEXT) Unknown", "2: str (TEXT) Unknown", "3: str (TEXT) Unknown"]),
    ("postgres", "SELECT 1 FROM orders WHERE ANY(%s) = NULL", &["1: None (None) Unknown"]),
    ("postgres", "SELECT 1 FROM orders WHERE %s = ANY(note)", &["1: None (None) Unknown"]),
    ("postgres", "SELECT 1 FROM orders WHERE ANY(%s, %s) = id", &["1: None (None) Unknown", "2: None (None) Unknown"]),
    ("postgres", "SELECT COALESCE(note, %s, 'x'), tag_count(%s, %s), tag_count(1, 2) FROM orders", &["1: str (TEXT) Nullable", "2: int (INT) Unknown", "3: None (None) Unknown"]),
];

/// `(dialect, query, columns as "name: type rule, nullability rule")`, against `SCHEMA`.
#[rustfmt::skip]
const EXPLAINED: &[Case] = &[
    ("postgres", "SELECT (id + 1), -id, SUM(id), id, (o).*, 1 FROM orders o", &["?column?: common.arithmetic.numbers, common.arithmetic.numbers", "?column?: -, -", "sum: postgres.aggregate.sum-integers, postgres.aggregate.sum-integers", "id: -, -", "id: -, -", "customer_id: -, -", "note: -, -", "?column?: -, -"]),
    ("postgres", "(SELECT SUM(id) FROM orders)", &["sum: postgres.aggregate.sum-integers, postgres.aggregate.sum-integers"]),
    ("postgres", "SELECT 1 UNION SELECT 2", &["?column?: -, -"]),
    ("mysql", "SELECT created + INTERVAL 1 DAY, id + 1 FROM (SELECT CURRENT_DATE AS created, 1 AS id) d", &["created + INTERVAL 1 DAY: -, -", "id + 1: common.arithmetic.numbers, common.arithmetic.numbers"]),
    ("postgres", "UPDATE orders SET note = 'x' RETURNING id + 1, *", &["?column?: common.arithmetic.numbers, common.arithmetic.numbers", "id: -, -", "customer_id: -, -", "note: -, -"]),
    ("postgres", "SELECT o.* FROM orders o", &["id: -, -", "customer_id: -, -", "note: -, -"]),
    ("postgres", "SELECT * FROM orders o, customers c", &["id: -, -", "customer_id: -, -", "note: -, -", "id: -, -", "email: -, -", "name: -, -"]),
    ("bigquery", "SELECT STRUCT(2 AS b).*", &["*: -, -"]),
    ("postgres", "DELETE FROM orders", &[]),
    ("postgres", "SELECT unknown_fn(1), lower('a')", &["unknown_fn: -, -", "lower: postgres.string.bound, postgres.string.bound"]),
    ("postgres", "SELECT id FROM orders ORDER BY id", &["id: -, -"]),
];

fn schema(dialect: &str) -> Schema {
    analyzer::load_schema_with_skipped(SCHEMA, dialect).0
}

/// Panics listing each case `read` disagrees with, as the row that would pass.
fn check(cases: &[Case], read: fn(&str, &str) -> Vec<String>) {
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|&(dialect, sql, expected)| {
            let actual = read(dialect, sql);
            (actual != expected).then(|| format!("    ({dialect:?}, {sql:?}, &{actual:?}),"))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} case(s) differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn found(dialect: &str, sql: &str) -> Vec<String> {
    let schema = schema(dialect);
    let (_, found) = analyzer::analyze_with_diagnostics(sql, dialect, Some(&schema));
    found
        .into_iter()
        .map(|d| format!("[{}] {}", d.rule.name(), d.message))
        .collect()
}

#[test]
fn each_query_reports_what_it_breaks() {
    check(CASES, found);
}

/// A column as `name: python (SQL)`, then the shape of a JSON column of known shape.
fn column(c: analyzer::Column) -> String {
    let python = c.display_type().unwrap_or_else(|| "None".into());
    let json = c
        .json
        .map(|shape| format!(" {}", shape.to_json()))
        .unwrap_or_default();
    let sql = c.sql_type.unwrap_or_else(|| "None".into());
    format!("{}: {python} ({sql}){json}", c.name)
}

fn columns(dialect: &str, sql: &str) -> Vec<String> {
    let schema = schema(dialect);
    analyzer::analyze_select(sql, dialect, Some(&schema))
        .unwrap_or_default()
        .into_iter()
        .map(column)
        .collect()
}

#[test]
fn each_column_has_its_type() {
    check(COLUMNS, columns);
}

/// A parameter as `index name: python (SQL) nullability`, then `cast` and the type an
/// untyped driver gets.
fn params(dialect: &str, sql: &str) -> Vec<String> {
    let schema = schema(dialect);
    analyzer::analyze_params(sql, dialect, Some(&schema))
        .unwrap_or_default()
        .into_iter()
        .map(|p| {
            let name = p.name.map(|name| format!(" {name}")).unwrap_or_default();
            let python = p.python_type.unwrap_or_else(|| "None".into());
            let sql = p.sql_type.unwrap_or_else(|| "None".into());
            let cast = if p.cast { " cast" } else { "" };
            let untyped = p
                .untyped_sql_type
                .map(|t| format!(" untyped {t}"))
                .unwrap_or_default();
            format!(
                "{}{name}: {python} ({sql}) {:?}{cast}{untyped}",
                p.index, p.nullability
            )
        })
        .collect()
}

#[test]
fn each_parameter_has_its_type() {
    check(PARAMS, params);
}

fn explained(dialect: &str, sql: &str) -> Vec<String> {
    let schema = schema(dialect);
    let id = |rule: Option<analyzer::RuleRef>| rule.map_or("-", |rule| rule.id);
    analyzer::explain_select(sql, dialect, Some(&schema))
        .unwrap_or_default()
        .into_iter()
        .map(|c| {
            format!(
                "{}: {}, {}",
                c.name,
                id(c.type_rule),
                id(c.nullability_rule)
            )
        })
        .collect()
}

#[test]
fn each_column_names_its_rules() {
    check(EXPLAINED, explained);
}

fn defined(dialect: &str, ddl: &str) -> Vec<String> {
    let (schema, skipped) = analyzer::load_schema_with_skipped(ddl, dialect);
    analyzer::analyze_select("SELECT * FROM t", dialect, Some(&schema))
        .unwrap_or_default()
        .into_iter()
        .map(column)
        .chain(
            skipped
                .into_iter()
                .map(|(kind, name)| format!("skipped {kind} {name}")),
        )
        .collect()
}

#[test]
fn each_schema_defines_its_table() {
    check(SCHEMAS, defined);
}
