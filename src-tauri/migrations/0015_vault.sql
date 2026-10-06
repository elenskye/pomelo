-- 芝麻开门 (passwords). Only ciphertext is stored: everything the user types for an entry lives in
-- vault_items.data (XChaCha20-Poly1305 under the vault key). The vault key is random; vault_meta keeps
-- it wrapped by a key derived from the master password (Argon2id). A copy for Touch ID sits in the
-- macOS keychain (readable without a prompt only by the build that wrote it: touch_build).
-- Neither table goes into data files (export); backups carry them as they are (encrypted).
CREATE TABLE vault_meta (
  id            INTEGER PRIMARY KEY CHECK (id = 1),
  kdf_salt      BLOB NOT NULL,
  kdf_m         INTEGER NOT NULL,             -- Argon2id memory (KiB)
  kdf_t         INTEGER NOT NULL,             -- iterations
  kdf_p         INTEGER NOT NULL,             -- lanes
  wrapped_key   BLOB NOT NULL,                -- nonce ‖ AEAD(master key, vault key)
  key_check     BLOB NOT NULL,                -- nonce ‖ AEAD(vault key, "pomelo-vault")
  master_at     TEXT NOT NULL,                -- last unlock with the master password (UTC)
  touch_build   TEXT,                         -- app build that saved the keychain copy
  touch_account TEXT,                         -- keychain account of that copy
  touch_locked  INTEGER NOT NULL DEFAULT 0,   -- Touch ID failed → master password first
  created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE vault_items (
  id         INTEGER PRIMARY KEY,
  data       BLOB NOT NULL,                   -- nonce ‖ AEAD(vault key, JSON of the fields)
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
