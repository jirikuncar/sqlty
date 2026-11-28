import postgres from "postgres";

const sql = postgres();

export async function user(id: string) {
  return sql`SELECT id, name, email FROM users WHERE id = ${id}`;
}

export async function spent(id: string) {
  return sql<{ name: string; total: number }[]>`
    SELECT u.name, sum(o.total) AS total
    FROM users u JOIN orders o ON o.user_id = u.id
    WHERE u.id = ${id}
    GROUP BY u.name
  `;
}

export async function recent() {
  return sql`SELECT id, emial FROM users ORDER BY created_at DESC LIMIT 10`;
}
