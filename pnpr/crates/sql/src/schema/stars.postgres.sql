CREATE TABLE IF NOT EXISTS npm_stars (
    namespace TEXT COLLATE "C" NOT NULL,
    name TEXT COLLATE "C" NOT NULL,
    user_id TEXT COLLATE "C" NOT NULL,
    PRIMARY KEY (namespace, name, user_id)
);
CREATE INDEX npm_stars_by_account ON npm_stars (user_id, name);
