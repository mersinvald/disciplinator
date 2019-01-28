use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Register {
    pub username: String,
    pub email: String,
    pub passwd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Login {
    pub username: String,
    pub passwd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUser {
    pub username: Option<String>,
    pub email: Option<String>,
    pub old_passwd: Option<String>,
    pub new_passwd: Option<String>,
}