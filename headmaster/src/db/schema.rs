table! {
    config (user_id) {
        user_id -> Int8,
        version -> Int4,
        hourly_activity_goal -> Int4,
        day_starts_at -> Time,
        day_ends_at -> Time,
        day_length -> Nullable<Int4>,
        hourly_debt_limit -> Nullable<Int4>,
        hourly_activity_limit -> Nullable<Int4>,
    }
}

table! {
    fitbit (user_id) {
        user_id -> Int8,
        client_id -> Varchar,
        client_secret -> Varchar,
        client_token -> Nullable<Varchar>,
    }
}

table! {
    users (id) {
        id -> Int8,
        username -> Varchar,
        email -> Varchar,
        email_verified -> Bool,
        passwd_hash -> Bytea,
    }
}

allow_tables_to_appear_in_same_query!(
    config,
    fitbit,
    users,
);
