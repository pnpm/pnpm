CREATE TABLE IF NOT EXISTS npm_stars (
    namespace VARBINARY(512) NOT NULL,
    name VARBINARY(512) NOT NULL,
    user_id VARBINARY(512) NOT NULL,
    PRIMARY KEY (namespace, name, user_id)
) ENGINE=InnoDB;
CREATE INDEX npm_stars_by_account ON npm_stars (user_id, name);
