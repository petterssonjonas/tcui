CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tabs (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    provider TEXT NOT NULL,
    endpoint TEXT,
    api_key_ref TEXT,
    api_key_storage TEXT,
    model TEXT NOT NULL,
    soul_name TEXT,
    agent_name TEXT,
    mcp_servers TEXT,
    tab_order INTEGER
);

CREATE TABLE IF NOT EXISTS file_backups (
    id INTEGER PRIMARY KEY,
    original_path TEXT NOT NULL,
    backup_path TEXT NOT NULL,
    message_id INTEGER
);

CREATE TABLE IF NOT EXISTS mcp_servers (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    transport TEXT NOT NULL,
    command TEXT,
    args TEXT,
    url TEXT,
    env TEXT,
    enabled INTEGER DEFAULT 1
);

CREATE TABLE IF NOT EXISTS key_metadata (
    id INTEGER PRIMARY KEY,
    salt TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS providers (
    name TEXT PRIMARY KEY,
    endpoint TEXT NOT NULL,
    env_var TEXT NOT NULL,
    backend_type TEXT NOT NULL DEFAULT 'openai',
    auth_type TEXT NOT NULL DEFAULT 'api_key'
);

CREATE TABLE IF NOT EXISTS models (
    id INTEGER PRIMARY KEY,
    provider TEXT NOT NULL,
    model_id TEXT NOT NULL,
    input_price REAL,
    output_price REAL,
    context_window INTEGER,
    fetched_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(provider, model_id)
);
