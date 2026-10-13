# RETURNING Columns

## UPDATE with RETURNING single column

```sql
UPDATE users SET name = 'John' WHERE id = 1 RETURNING id::INTEGER
-- columns: id: int (INT)
```

## UPDATE with RETURNING multiple columns

```sql
UPDATE users SET name = 'John' WHERE id = 1 RETURNING id::INTEGER, name::TEXT
-- columns: id: int (INT)
-- columns: name: str (TEXT)
```

## UPDATE with RETURNING various types

```sql
UPDATE users
SET name = 'John', age = 30, active = true
WHERE id = 1
RETURNING id::INTEGER, name::TEXT, age::INTEGER, active::BOOLEAN, updated_at::TIMESTAMP
-- columns: id: int (INT)
-- columns: name: str (TEXT)
-- columns: age: int (INT)
-- columns: active: bool (BOOLEAN)
-- columns: updated_at: datetime.datetime (TIMESTAMP)
```

## UPDATE with RETURNING *

```sql
UPDATE users SET name = 'John' RETURNING *
-- columns: *: None (None)
```

## Generate_return_type for UPDATE with RETURNING

```sql
UPDATE users SET name = 'John' RETURNING id::INTEGER, name::TEXT
-- inferred: tuple[int, str]
```

## DELETE with RETURNING single column

```sql
DELETE FROM users WHERE id = 1 RETURNING id::INTEGER
-- columns: id: int (INT)
```

## DELETE with RETURNING multiple columns

```sql
DELETE FROM users WHERE active = false RETURNING id::INTEGER, name::TEXT, email::TEXT
-- columns: id: int (INT)
-- columns: name: str (TEXT)
-- columns: email: str (TEXT)
```

## DELETE with RETURNING various types

```sql
DELETE FROM orders
WHERE status = 'cancelled'
RETURNING order_id::INTEGER, total::FLOAT, created_at::TIMESTAMP, cancelled::BOOLEAN
-- columns: order_id: int (INT)
-- columns: total: float (DOUBLE)
-- columns: created_at: datetime.datetime (TIMESTAMP)
-- columns: cancelled: bool (BOOLEAN)
```

## Generate_return_type for DELETE with RETURNING

```sql
DELETE FROM users WHERE id = 1 RETURNING id::INTEGER
-- inferred: tuple[int]
```

## INSERT with RETURNING single column

```sql
INSERT INTO users (name) VALUES ('John') RETURNING id::INTEGER
-- columns: id: int (INT)
```

## INSERT with RETURNING multiple columns

```sql
INSERT INTO users (name, email)
VALUES ('John', 'john@example.com')
RETURNING id::INTEGER, name::TEXT, created_at::TIMESTAMP
-- columns: id: int (INT)
-- columns: name: str (TEXT)
-- columns: created_at: datetime.datetime (TIMESTAMP)
```

## INSERT with multiple VALUES and RETURNING

```sql
INSERT INTO users (name, email)
VALUES
    ('John', 'john@example.com'),
    ('Jane', 'jane@example.com')
RETURNING id::INTEGER, name::TEXT
-- columns: id: int (INT)
-- columns: name: str (TEXT)
```

## Generate_return_type for INSERT with RETURNING

```sql
INSERT INTO users (name) VALUES ('John') RETURNING id::INTEGER, name::TEXT
-- inferred: tuple[int, str]
```

## MERGE with RETURNING single column

```sql
MERGE INTO users
USING updates ON users.id = updates.id
WHEN MATCHED THEN UPDATE SET name = updates.name
WHEN NOT MATCHED THEN INSERT (name) VALUES (updates.name)
RETURNING id::INTEGER
-- columns: id: int (INT)
```

## MERGE with RETURNING multiple columns

```sql
MERGE INTO users
USING updates ON users.id = updates.id
WHEN MATCHED THEN UPDATE SET name = updates.name, email = updates.email
WHEN NOT MATCHED THEN INSERT (name, email) VALUES (updates.name, updates.email)
RETURNING id::INTEGER, name::TEXT, email::TEXT
-- columns: id: int (INT)
-- columns: name: str (TEXT)
-- columns: email: str (TEXT)
```

## Generate_return_type for MERGE with RETURNING

```sql
MERGE INTO users
USING updates ON users.id = updates.id
WHEN MATCHED THEN UPDATE SET name = updates.name
RETURNING id::INTEGER, name::TEXT
-- inferred: tuple[int, str]
```

## UPDATE without WHERE clause with RETURNING

```sql
UPDATE users SET active = false RETURNING id::INTEGER, active::BOOLEAN
-- columns: id: int (INT)
-- columns: active: bool (BOOLEAN)
```

## DELETE with RETURNING *

```sql
DELETE FROM users WHERE active = false RETURNING *
-- columns: *: None (None)
```

## UPDATE with RETURNING expressions

```sql
UPDATE products
SET price = price * 1.1
RETURNING id::INTEGER, (price * 1.1)::FLOAT
-- columns: id: int (INT)
-- columns: float8: float (DOUBLE)
```

## RETURNING with various datetime types

```sql
UPDATE events
SET status = 'completed'
RETURNING
    id::INTEGER,
    created_at::TIMESTAMP,
    completed_at::TIMESTAMPTZ,
    scheduled_date::DATE,
    scheduled_time::TIME
-- columns: id: int (INT)
-- columns: created_at: datetime.datetime (TIMESTAMP)
-- columns: completed_at: datetime.datetime (TIMESTAMPTZ)
-- columns: scheduled_date: datetime.date (DATE)
-- columns: scheduled_time: datetime.time (TIME)
```

## RETURNING with UUID type

```sql
INSERT INTO users (name) VALUES ('John') RETURNING user_id::UUID, name::TEXT
-- columns: user_id: uuid.UUID (UUID)
-- columns: name: str (TEXT)
```

## RETURNING with JSON types

```sql
UPDATE settings
SET config = '{"key": "value"}'::jsonb
RETURNING id::INTEGER, config::JSONB
-- columns: id: int (INT)
-- columns: config: dict (JSONB)
```

## UPDATE with FROM clause and RETURNING

```sql
UPDATE users
SET status = 'active'
FROM orders
WHERE users.id = orders.user_id AND orders.total > 100
RETURNING users.id::INTEGER, users.status::TEXT
-- columns: id: int (INT)
-- columns: status: str (TEXT)
```

## DELETE with USING clause and RETURNING

```sql
DELETE FROM orders
USING users
WHERE orders.user_id = users.id AND users.active = false
RETURNING orders.id::INTEGER, orders.total::FLOAT
-- columns: id: int (INT)
-- columns: total: float (DOUBLE)
```

## UPDATE without RETURNING returns None

```sql
UPDATE users SET name = 'John' WHERE id = 1
-- columns: None
```

## DELETE without RETURNING returns None

```sql
DELETE FROM users WHERE id = 1
-- columns: None
```

## INSERT without RETURNING returns None

```sql
INSERT INTO users (name) VALUES ('John')
-- columns: None
```

## UPDATE...FROM where RETURNING references columns from both tables

```sql
UPDATE users
SET status = orders.status
FROM orders
WHERE users.id = orders.user_id
RETURNING users.id::INTEGER, orders.order_id::INTEGER, users.status::TEXT
-- columns: id: int (INT)
-- columns: order_id: int (INT)
-- columns: status: str (TEXT)
```

## DELETE...USING where RETURNING references columns from both tables

```sql
DELETE FROM orders
USING users, products
WHERE orders.user_id = users.id
AND orders.product_id = products.id
AND users.active = false
RETURNING orders.id::INTEGER, users.email::TEXT, products.name::TEXT
-- columns: id: int (INT)
-- columns: email: str (TEXT)
-- columns: name: str (TEXT)
```

## UPDATE with subquery in SET clause and RETURNING

```sql
UPDATE users
SET total_orders = (SELECT COUNT(*) FROM orders WHERE orders.user_id = users.id)
WHERE active = true
RETURNING id::INTEGER, total_orders::INTEGER
-- columns: id: int (INT)
-- columns: total_orders: int (INT)
```

## UPDATE with JOIN in FROM clause and RETURNING

```sql
UPDATE users
SET tier = 'premium'
FROM (
    SELECT user_id, SUM(total) as total_spent
    FROM orders
    GROUP BY user_id
    HAVING SUM(total) > 1000
) AS high_spenders
WHERE users.id = high_spenders.user_id
RETURNING users.id::INTEGER, users.tier::TEXT, high_spenders.total_spent::FLOAT
-- columns: id: int (INT)
-- columns: tier: str (TEXT)
-- columns: total_spent: float (DOUBLE)
```

## DELETE with CTE (WITH clause) and RETURNING

```sql
WITH inactive_users AS (
    SELECT id FROM users WHERE last_login < NOW() - INTERVAL '1 year'
)
DELETE FROM orders
WHERE user_id IN (SELECT id FROM inactive_users)
RETURNING id::INTEGER, user_id::INTEGER, created_at::TIMESTAMP
-- columns: id: int (INT)
-- columns: user_id: int (INT)
-- columns: created_at: datetime.datetime (TIMESTAMP)
```

## UPDATE RETURNING with expressions and calculations

```sql
UPDATE products
SET price = price * 1.1, updated_at = NOW()
RETURNING id::INTEGER, (price * 1.1)::FLOAT AS new_price, UPPER(name)::TEXT AS upper_name
-- columns: id: int (INT)
-- columns: new_price: float (DOUBLE)
-- columns: upper_name: str (TEXT)
```

## INSERT...SELECT with RETURNING

```sql
INSERT INTO audit_log (user_id, action, timestamp)
SELECT id, 'login', NOW()
FROM users
WHERE active = true
RETURNING id::INTEGER, user_id::INTEGER, action::TEXT, timestamp::TIMESTAMP
-- columns: id: int (INT)
-- columns: user_id: int (INT)
-- columns: action: str (TEXT)
-- columns: timestamp: datetime.datetime (TIMESTAMP)
```

## UPDATE with multiple JOINs in FROM and RETURNING

```sql
UPDATE order_items
SET discount = discounts.percentage
FROM orders
INNER JOIN users ON orders.user_id = users.id
INNER JOIN discounts ON users.tier = discounts.tier
WHERE order_items.order_id = orders.id
RETURNING order_items.id::INTEGER, order_items.discount::FLOAT, users.tier::TEXT
-- columns: id: int (INT)
-- columns: discount: float (DOUBLE)
-- columns: tier: str (TEXT)
```

## DELETE with nested SELECT in WHERE and RETURNING

```sql
DELETE FROM users
WHERE id IN (
    SELECT user_id FROM orders
    WHERE status = 'cancelled'
    AND created_at < (
        SELECT MIN(created_at) FROM orders WHERE status = 'completed'
    )
)
RETURNING id::INTEGER, email::TEXT, created_at::TIMESTAMP
-- columns: id: int (INT)
-- columns: email: str (TEXT)
-- columns: created_at: datetime.datetime (TIMESTAMP)
```

## UPDATE RETURNING with CASE expression

```sql
UPDATE users
SET tier = CASE
    WHEN total_spent > 10000 THEN 'platinum'
    WHEN total_spent > 5000 THEN 'gold'
    WHEN total_spent > 1000 THEN 'silver'
    ELSE 'bronze'
END
RETURNING id::INTEGER, tier::TEXT, total_spent::FLOAT
-- columns: id: int (INT)
-- columns: tier: str (TEXT)
-- columns: total_spent: float (DOUBLE)
```

## MERGE with complex WHEN conditions and RETURNING

```sql
MERGE INTO inventory
USING (
    SELECT product_id, SUM(quantity) as qty
    FROM order_items
    GROUP BY product_id
) AS orders
ON inventory.product_id = orders.product_id
WHEN MATCHED AND orders.qty > 0 THEN
    UPDATE SET quantity = inventory.quantity - orders.qty
WHEN NOT MATCHED THEN
    INSERT (product_id, quantity) VALUES (orders.product_id, 0)
RETURNING inventory.product_id::INTEGER, inventory.quantity::INTEGER, orders.qty::INTEGER
-- columns: product_id: int (INT)
-- columns: quantity: int (INT)
-- columns: qty: int (INT)
```

## UPDATE with LATERAL join in FROM and RETURNING

```sql
UPDATE users
SET last_order_date = latest.order_date
FROM LATERAL (
    SELECT MAX(created_at) as order_date
    FROM orders
    WHERE orders.user_id = users.id
) AS latest
WHERE users.active = true
RETURNING users.id::INTEGER, users.last_order_date::DATE
-- columns: id: int (INT)
-- columns: last_order_date: datetime.date (DATE)
```

## DELETE RETURNING with window function in expression

```sql
DELETE FROM duplicates
WHERE id NOT IN (
    SELECT id FROM (
        SELECT id, ROW_NUMBER() OVER (PARTITION BY email ORDER BY created_at DESC) as rn
        FROM duplicates
    ) t WHERE rn = 1
)
RETURNING id::INTEGER, email::TEXT, created_at::TIMESTAMP
-- columns: id: int (INT)
-- columns: email: str (TEXT)
-- columns: created_at: datetime.datetime (TIMESTAMP)
```

## INSERT...ON CONFLICT with RETURNING

```sql
INSERT INTO users (email, name, login_count)
VALUES ('user@example.com', 'User', 1)
ON CONFLICT (email) DO UPDATE
SET login_count = users.login_count + 1, last_login = NOW()
RETURNING id::INTEGER, email::TEXT, login_count::INTEGER, last_login::TIMESTAMP
-- columns: id: int (INT)
-- columns: email: str (TEXT)
-- columns: login_count: int (INT)
-- columns: last_login: datetime.datetime (TIMESTAMP)
```
