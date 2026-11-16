#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Calendar(#[from] anlg_calendar::Error),
    #[error("auth error: {0}")]
    Auth(String),
    /// An account connected on this Mac failed (needs reconnecting, network…).
    #[error("account error ({code}): {message}")]
    Account { code: &'static str, message: String },
}

impl From<anlg_local_accounts::Error> for Error {
    fn from(error: anlg_local_accounts::Error) -> Self {
        Error::Account {
            code: error.code(),
            message: error.to_string(),
        }
    }
}

impl serde::Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl specta::Type for Error {
    fn inline(_type_map: &mut specta::TypeMap, _generics: specta::Generics) -> specta::DataType {
        specta::DataType::Primitive(specta::datatype::PrimitiveType::String)
    }
}
