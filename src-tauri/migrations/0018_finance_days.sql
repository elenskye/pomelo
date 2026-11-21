-- 钱来钱往, round 5:
-- 1. Balances, rates and holdings are dated ('YYYY-MM-DD', were monthly 'YYYY-MM'). An old month becomes
--    the local day it was last saved when that falls inside the month, else the month's last day
--    (first day if saved before the month started).
-- 2. Holdings store quantity + unit price (market value = quantity × price). Old rows: price = value ÷
--    quantity; without a quantity → quantity 1, price = value.
-- 3. 收支 entries can cover a period (day … end_day; reports spread the amount evenly over its days).
-- 4. Entries and 固定支出 no longer have an account.

ALTER TABLE fin_balances RENAME COLUMN month TO day;
UPDATE fin_balances SET day = CASE
  WHEN substr(date(updated_at, 'localtime'), 1, 7) = day THEN date(updated_at, 'localtime')
  WHEN date(updated_at, 'localtime') < day || '-01' THEN day || '-01'
  ELSE date(day || '-01', '+1 month', '-1 day') END;

ALTER TABLE fin_rates RENAME COLUMN month TO day;
UPDATE fin_rates SET day = CASE
  WHEN substr(date(updated_at, 'localtime'), 1, 7) = day THEN date(updated_at, 'localtime')
  WHEN date(updated_at, 'localtime') < day || '-01' THEN day || '-01'
  ELSE date(day || '-01', '+1 month', '-1 day') END;

-- fin_holdings has no children: rebuild in place.
CREATE TABLE fin_holdings_new (
  id         INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES fin_accounts(id) ON DELETE CASCADE,
  day        TEXT NOT NULL, -- YYYY-MM-DD
  name       TEXT NOT NULL,
  quantity   REAL NOT NULL,
  price      REAL NOT NULL, -- unit price in the account's currency (not cents)
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
INSERT INTO fin_holdings_new (id, account_id, day, name, quantity, price, sort_order, created_at, updated_at)
  SELECT id, account_id,
    CASE
      WHEN substr(date(updated_at, 'localtime'), 1, 7) = month THEN date(updated_at, 'localtime')
      WHEN date(updated_at, 'localtime') < month || '-01' THEN month || '-01'
      ELSE date(month || '-01', '+1 month', '-1 day') END,
    name,
    CASE WHEN quantity > 0 THEN quantity ELSE 1.0 END,
    CASE WHEN quantity > 0 THEN value / 100.0 / quantity ELSE value / 100.0 END,
    sort_order, created_at, updated_at
  FROM fin_holdings;
DROP TABLE fin_holdings;
ALTER TABLE fin_holdings_new RENAME TO fin_holdings;
CREATE INDEX fin_holdings_by_account ON fin_holdings(account_id, day, sort_order);

-- fin_fixed is fin_entries' parent: build both new tables, drop the old child, then the old parent
-- (nothing references it any more, so no ON DELETE action fires), then rename (FKs follow the rename).
CREATE TABLE fin_fixed_new (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE,
  flow       TEXT NOT NULL DEFAULT 'out', -- out | in
  amount     INTEGER NOT NULL,
  currency   TEXT NOT NULL,
  kind_id    INTEGER REFERENCES kinds(id) ON DELETE SET NULL,
  rule       TEXT NOT NULL, -- monthly:<1-31> | yearly:<MM-DD>
  next_day   TEXT NOT NULL, -- YYYY-MM-DD
  stopped    INTEGER NOT NULL DEFAULT 0 CHECK (stopped IN (0, 1)),
  note       TEXT,
  sort_order INTEGER NOT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
INSERT INTO fin_fixed_new (id, name, flow, amount, currency, kind_id, rule, next_day, stopped, note, sort_order, created_at, updated_at)
  SELECT id, name, flow, amount, currency, kind_id, rule, next_day, stopped, note, sort_order, created_at, updated_at FROM fin_fixed;

CREATE TABLE fin_entries_new (
  id         INTEGER PRIMARY KEY,
  flow       TEXT NOT NULL, -- out | in
  amount     INTEGER NOT NULL CHECK (amount > 0),
  currency   TEXT NOT NULL,
  day        TEXT NOT NULL, -- YYYY-MM-DD (a period's first day)
  end_day    TEXT,          -- a period's last day (after day); NULL = just `day`
  kind_id    INTEGER REFERENCES kinds(id) ON DELETE SET NULL,
  fixed_id   INTEGER REFERENCES fin_fixed_new(id) ON DELETE SET NULL, -- posted by a 固定支出
  note       TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
INSERT INTO fin_entries_new (id, flow, amount, currency, day, kind_id, fixed_id, note, created_at, updated_at)
  SELECT id, flow, amount, currency, day, kind_id, fixed_id, note, created_at, updated_at FROM fin_entries;

DROP TABLE fin_entries;
DROP TABLE fin_fixed;
ALTER TABLE fin_fixed_new RENAME TO fin_fixed;
ALTER TABLE fin_entries_new RENAME TO fin_entries;
CREATE INDEX fin_entries_by_day ON fin_entries(day);
CREATE INDEX fin_entries_by_kind ON fin_entries(kind_id);
