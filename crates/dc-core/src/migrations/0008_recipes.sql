-- Recipes: a search + actions, run on demand or on newly imported tracks.
CREATE TABLE recipes (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    query      TEXT NOT NULL DEFAULT '',
    actions    TEXT NOT NULL,          -- JSON array of recipes::Action
    on_import  INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    last_run   INTEGER
);
