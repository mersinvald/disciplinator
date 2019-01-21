table! {
    fitbit (user_id) {
        user_id -> Int8,
        client_id -> Varchar,
        client_secret -> Varchar,
        client_token -> Nullable<Varchar>,
    }
}

table! {
    settings (user_id) {
        user_id -> Int8,
        hourly_activity_goal -> Int4,
        day_starts_at -> Time,
        day_ends_at -> Time,
        day_length -> Nullable<Int4>,
        hourly_debt_limit -> Nullable<Int4>,
        hourly_activity_limit -> Nullable<Int4>,
    }
}

table! {
    summary_cache (user_id) {
        user_id -> Int8,
        created_at -> Timestamptz,
        summary -> Text,
    }
}

table! {
    tokens (token) {
        token -> Uuid,
        user_id -> Int8,
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
    fitbit,
    settings,
    summary_cache,
    tokens,
    users,
);
