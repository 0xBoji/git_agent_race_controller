use thiserror::Error;

#[derive(Debug, Error)]
pub enum GarcError {
    #[error("CAMP config `{path}` is missing; run `camp init --force` in this repository first")]
    MissingCampConfig { path: String },
    #[error(
        "CAMP project `{config_project}` from `{config_path}` does not match repository project `{repo_project}`; update `.camp.toml` or re-run `camp init` for this repo"
    )]
    ProjectMismatch {
        config_path: String,
        config_project: String,
        repo_project: String,
    },
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
