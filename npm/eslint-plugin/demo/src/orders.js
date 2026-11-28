import postgres from "postgres";

const sql = postgres();

export async function largest(userId) {
  return sql`SELECT id, totl FROM orders WHERE user_id = ${userId} ORDER BY total DESC`;
}

export async function owners() {
  return sql`SELECT o.id, u.name FROM orders o JOIN user u ON u.id = o.user_id`;
}
