use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LinkkitError {
    #[error("XML 解析错误: {0}")]
    ParseError(String),

    #[error("标签未闭合: {tag}")]
    UnclosedTag { tag: String },

    #[error("未知标签: {tag}")]
    UnknownTag { tag: String },

    #[error("缺少必需属性: {attr} in <{tag}>")]
    MissingAttribute { tag: String, attr: String },

    #[error("无效的属性值: {attr}={value} in <{tag}>")]
    InvalidAttributeValue {
        tag: String,
        attr: String,
        value: String,
    },

    #[error("标签嵌套错误: 期望 </{expected}> 但遇到 </{found}>")]
    NestingMismatch { expected: String, found: String },

    #[error("标签内容为空: <{tag}>")]
    EmptyContent { tag: String },

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("quick-xml 错误: {0}")]
    QuickXml(#[from] quick_xml::Error),

    #[error("quick-xml 属性错误: {0}")]
    QuickXmlAttr(#[from] quick_xml::events::attributes::AttrError),

    #[error("UTF-8 解码错误: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    #[error("JSON 序列化错误: {0}")]
    Json(#[from] serde_json::Error),

    #[error("其他错误: {0}")]
    Other(String),
}

#[derive(Debug, Error)]
pub enum GateError {
    #[error("read-gate: 必须先读取文件才能编辑: {0}")]
    MustReadFirst(PathBuf),

    #[error("doc-gate: 必须先读取工具文档才能调用: {0}")]
    MustReadDocFirst(String),

    #[error("权限拒绝: {0}")]
    PermissionDenied(String),
}

pub type LinkkitResult<T> = Result<T, LinkkitError>;
