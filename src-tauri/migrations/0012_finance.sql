-- 钱来钱往 (finance). Amounts are integer cents in the row's currency (CNY | SGD | HKD | USD).
-- 分类 = kinds (module 'fin_out' 支出 / 'fin_in' 收入).
CREATE TABLE fin_accounts (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE,
  kind       TEXT NOT NULL DEFAULT 'bank', -- bank | wallet | broker
  currency   TEXT NOT NULL,                -- one currency per account
  closed     INTEGER NOT NULL DEFAULT 0 CHECK (closed IN (0, 1)),
  note       TEXT,
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- Monthly snapshots (a broker account's balance = its total value).
CREATE TABLE fin_balances (
  id         INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES fin_accounts(id) ON DELETE CASCADE,
  month      TEXT NOT NULL, -- YYYY-MM
  amount     INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE (account_id, month)
) STRICT;

-- How many CNY one unit of `currency` was worth that month (CNY itself is 1, not stored).
CREATE TABLE fin_rates (
  id         INTEGER PRIMARY KEY,
  month      TEXT NOT NULL,
  currency   TEXT NOT NULL,
  rate       REAL NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE (month, currency)
) STRICT;

CREATE TABLE fin_holdings (
  id         INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES fin_accounts(id) ON DELETE CASCADE,
  month      TEXT NOT NULL,
  name       TEXT NOT NULL,
  quantity   REAL,
  value      INTEGER NOT NULL, -- market value, account currency
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX fin_holdings_by_account ON fin_holdings(account_id, month, sort_order);

-- 固定支出 (or fixed income): posts an entry on next_day, then moves next_day on by the rule.
CREATE TABLE fin_fixed (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE,
  flow       TEXT NOT NULL DEFAULT 'out', -- out | in
  amount     INTEGER NOT NULL,
  currency   TEXT NOT NULL,
  kind_id    INTEGER REFERENCES kinds(id) ON DELETE SET NULL,
  account_id INTEGER REFERENCES fin_accounts(id) ON DELETE SET NULL,
  rule       TEXT NOT NULL, -- monthly:<1-31> | yearly:<MM-DD>
  next_day   TEXT NOT NULL, -- YYYY-MM-DD
  stopped    INTEGER NOT NULL DEFAULT 0 CHECK (stopped IN (0, 1)),
  note       TEXT,
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE fin_entries (
  id         INTEGER PRIMARY KEY,
  flow       TEXT NOT NULL, -- out | in
  amount     INTEGER NOT NULL CHECK (amount > 0),
  currency   TEXT NOT NULL,
  day        TEXT NOT NULL, -- YYYY-MM-DD
  kind_id    INTEGER REFERENCES kinds(id) ON DELETE SET NULL,
  account_id INTEGER REFERENCES fin_accounts(id) ON DELETE SET NULL,
  fixed_id   INTEGER REFERENCES fin_fixed(id) ON DELETE SET NULL, -- posted by a 固定支出
  note       TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX fin_entries_by_day ON fin_entries(day);
CREATE INDEX fin_entries_by_kind ON fin_entries(kind_id);

-- Monthly budget per 支出分类 and currency.
CREATE TABLE fin_budgets (
  id         INTEGER PRIMARY KEY,
  kind_id    INTEGER NOT NULL REFERENCES kinds(id) ON DELETE CASCADE,
  currency   TEXT NOT NULL,
  amount     INTEGER NOT NULL CHECK (amount > 0),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE (kind_id, currency)
) STRICT;

INSERT INTO kinds (module, name, sort_order) VALUES
  ('fin_out', '餐饮', 1), ('fin_out', '交通', 2), ('fin_out', '购物', 3), ('fin_out', '住房', 4),
  ('fin_out', '通讯', 5), ('fin_out', '订阅', 6), ('fin_out', '学习', 7), ('fin_out', '娱乐', 8),
  ('fin_out', '医疗', 9), ('fin_out', '旅行', 10), ('fin_out', '其他', 11),
  ('fin_in', '津贴', 1), ('fin_in', '奖学金', 2), ('fin_in', '家里', 3), ('fin_in', '投资收益', 4), ('fin_in', '其他', 5);
