-- Add migration script here
CREATE TABLE RPGCharacter (
    user_id TEXT NOT NULL PRIMARY KEY,
    losses INTEGER NOT NULL DEFAULT 0,
    wins INTEGER NOT NULL DEFAULT 0,
    draws INTEGER NOT NULL DEFAULT 0,
    last_loss DATETIME NOT NULL DEFAULT 0,
    elo_rank INTEGER NOT NULL DEFAULT 1000,
    peak_elo INTEGER NOT NULL DEFAULT 1000,
    floor_elo INTEGER NOT NULL DEFAULT 1000
);

CREATE TABLE RPGFight (
    message_id TEXT NOT NULL PRIMARY KEY,
    log TEXT NOT NULL
);