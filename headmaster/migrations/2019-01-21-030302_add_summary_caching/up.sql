-- Your SQL goes here
CREATE TABLE IF NOT EXISTS summary_cache
(
   user_id BIGINT NOT NULL,
   created_at TIMESTAMP WITH TIME ZONE NOT NULL,
   summary TEXT NOT NULL,
   PRIMARY KEY(user_id, created_at)
);