//! GUIとローカルAPIで共有する操作要求と受付結果。
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub action: String,
    pub token: Option<String>,
    pub axis: Option<String>,
    pub value: Option<f32>,
    pub flag: Option<bool>,
    pub text: Option<String>,
}
impl Request {
    pub fn new(action: &str) -> Self {
        Self {
            action: action.into(),
            ..Default::default()
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    pub message: String,
    pub data: String,
    pub token: Option<String>,
}
impl Reply {
    pub fn accepted() -> Self {
        Self {
            ok: true,
            message: "受付済み。基板の反映はstatusで確認してください".into(),
            data: String::new(),
            token: None,
        }
    }
    pub fn data(data: String) -> Self {
        Self {
            ok: true,
            message: "確認済み".into(),
            data,
            token: None,
        }
    }
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            data: String::new(),
            token: None,
        }
    }
}
