use thiserror::Error;

#[derive(Debug, Error)]
pub enum GarcError {
    #[error("CAMP config `{path}` is missing; run `camp init --force` in this repository first")]
    MissingCampConfig { path: String },
    #[error("resolved mesh service `{fullname}` is missing required TXT field `{field}`")]
    MissingTxtField {
        fullname: String,
        field: &'static str,
    },
    #[error("resolved mesh service `{fullname}` contains invalid UTF-8 in `{field}`")]
    InvalidTxtFieldEncoding {
        fullname: String,
        field: &'static str,
    },
}
