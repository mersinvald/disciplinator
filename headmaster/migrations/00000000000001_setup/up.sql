-- Your SQL goes here
-- Create tables
CREATE TABLE IF NOT EXISTS users
(
    ID BIGSERIAL NOT NULL UNIQUE ,
    username VARCHAR(128) NOT NULL UNIQUE,
    email VARCHAR(128) NOT NULL UNIQUE,
    email_verified BOOLEAN NOT NULL,
    passwd_hash BYTEA NOT NULL,
    PRIMARY KEY(ID)
);

CREATE TABLE IF NOT EXISTS config
(
    user_id BIGINT NOT NULL,
    version INTEGER NOT NULL,
    hourly_activity_goal INTEGER NOT NULL,
    day_starts_at TIME NOT NULL,
    day_ends_at TIME NOT NULL,
    day_length INTEGER,
    hourly_debt_limit INTEGER,
    hourly_activity_limit INTEGER,
    PRIMARY KEY(user_id)
);

CREATE TABLE IF NOT EXISTS fitbit
(
    user_id BIGINT NOT NULL,
    client_id VARCHAR(32) NOT NULL,
    client_secret VARCHAR(128) NOT NULL,
    client_token VARCHAR(1024),
    PRIMARY KEY(user_id)
);

CREATE TABLE IF NOT EXISTS tokens
(
    token UUID NOT NULL,
    user_id BIGINT NOT NULL,
    PRIMARY KEY(token)
);