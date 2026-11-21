-- The typical queries one check is budgeted on (AGENTS.md "Check budget"), against the
-- tables t0000-t0042 of synth.py and sqlty-bench: `one_query_us` of bench_project.py and
-- the Criterion group `one_query` read this file. Each is a `-- name:` line, then the query.

-- name: point_select
SELECT id, name, status, created_at FROM app.t0042 WHERE id = $1

-- name: join_3
SELECT a.id, b.name, c.score FROM app.t0002 a JOIN app.t0001 b ON a.parent_id = b.id
JOIN app.t0000 c ON b.parent_id = c.id WHERE a.status = $1

-- name: insert_params
INSERT INTO app.t0042 (id, parent_id, name, score, tags, payload)
VALUES ($1, $2, $3, $4, $5, $6) RETURNING id, created_at

-- name: aggregate
SELECT status, count(*) AS n, sum(score) AS total, max(created_at) AS last
FROM app.t0042 WHERE score > $1 GROUP BY status HAVING count(*) > 1
