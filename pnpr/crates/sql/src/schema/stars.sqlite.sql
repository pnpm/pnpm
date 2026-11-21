CREATE TABLE IF NOT EXISTS npm_stars (
    namespace TEXT NOT NULL,
    name TEXT NOT NULL,
    user_id TEXT NOT NULL,
    PRIMARY KEY (namespace, name, user_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX npm_stars_by_account ON npm_stars (user_id, name);
