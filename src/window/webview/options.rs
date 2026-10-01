use taurino_core::{
    anyhow,
    serde::{Deserialize, Serialize},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "taurino_core::serde", rename_all = "camelCase")]
pub struct WebViewOptions {
    pub url: String,
}
