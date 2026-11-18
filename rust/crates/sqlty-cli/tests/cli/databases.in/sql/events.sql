-- name: get_kinds
SELECT nope FROM events;

-- name: by-kind^
SELECT kind FROM events WHERE kind = :kind;
