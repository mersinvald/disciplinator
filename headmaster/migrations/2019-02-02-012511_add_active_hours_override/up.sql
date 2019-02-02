-- Your SQL goes here
CREATE TABLE active_hours_overrides
(
    user_id BIGINT NOT NULL,
    override_date DATE NOT NULL,
    override_hour INT NOT NULL,
    is_active BOOLEAN NOT NULL,
    PRIMARY KEY(user_id, override_date, override_hour)
);