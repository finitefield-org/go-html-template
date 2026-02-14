use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::HashSet;
#[cfg(not(feature = "web-rust"))]
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

use serde::Serialize;
use serde_json::Value as JsonValue;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, TemplateError>;

#[cfg(not(feature = "web-rust"))]
pub trait TemplateFS {
    fn read_file(&self, path: &str) -> Result<Vec<u8>>;
    fn glob(&self, pattern: &str) -> Result<Vec<String>>;
}

#[cfg(not(feature = "web-rust"))]
#[derive(Clone, Debug)]
pub struct OSFileSystem;

#[cfg(not(feature = "web-rust"))]
impl TemplateFS for OSFileSystem {
    fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        Ok(fs::read(path)?)
    }

    fn glob(&self, pattern: &str) -> Result<Vec<String>> {
        let mut paths = Vec::new();
        for entry in glob::glob(pattern)? {
            paths.push(entry?.to_string_lossy().to_string());
        }

        if paths.is_empty() {
            return Err(TemplateError::Parse(format!(
                "glob pattern matched no files: {pattern}"
            )));
        }

        Ok(paths)
    }
}

#[derive(Debug, Error)]
pub enum TemplateError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("glob pattern error: {0}")]
    GlobPattern(#[from] glob::PatternError),
    #[error("glob error: {0}")]
    Glob(#[from] glob::GlobError),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("render error: {0}")]
    Render(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateErrorInfo {
    pub line: Option<usize>,
    pub name: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateErrorCode {
    ErrBadHTML,
    ErrBranchEnd,
    ErrAmbigContext,
    ErrEndContext,
    ErrNoSuchTemplate,
    ErrOutputContext,
    ErrRangeLoopReentry,
    ErrPartialCharset,
    ErrPartialEscape,
    ErrSlashAmbig,
    ErrPredefinedEscaper,
    ErrMissingKey,
    ErrNotDefined,
    ErrInvalidUTF8,
    ErrParse,
    ErrRender,
    ErrFileSystem,
    ErrInternal,
    ErrOther,
}

impl TemplateError {
    pub fn code(&self) -> TemplateErrorCode {
        match self {
            TemplateError::Io(_) => TemplateErrorCode::ErrFileSystem,
            TemplateError::Json(_) => TemplateErrorCode::ErrInternal,
            TemplateError::GlobPattern(_) => TemplateErrorCode::ErrFileSystem,
            TemplateError::Glob(_) => TemplateErrorCode::ErrFileSystem,
            TemplateError::Parse(message) => parse_error_code(message),
            TemplateError::Render(message) => render_error_code(message),
        }
    }

    pub fn info(&self) -> TemplateErrorInfo {
        match self {
            TemplateError::Parse(message) | TemplateError::Render(message) => TemplateErrorInfo {
                line: parse_error_line(message),
                name: parse_error_name(message),
                reason: message.clone(),
            },
            TemplateError::Io(error) => TemplateErrorInfo {
                line: None,
                name: None,
                reason: error.to_string(),
            },
            TemplateError::Json(error) => TemplateErrorInfo {
                line: None,
                name: None,
                reason: error.to_string(),
            },
            TemplateError::GlobPattern(error) => TemplateErrorInfo {
                line: None,
                name: None,
                reason: error.to_string(),
            },
            TemplateError::Glob(error) => TemplateErrorInfo {
                line: None,
                name: None,
                reason: error.to_string(),
            },
        }
    }

    pub fn line(&self) -> Option<usize> {
        self.info().line
    }

    pub fn name(&self) -> Option<String> {
        self.info().name
    }

    pub fn reason(&self) -> String {
        self.info().reason
    }
}

fn parse_error_code(message: &str) -> TemplateErrorCode {
    if message.contains("branches end in different contexts") {
        TemplateErrorCode::ErrBranchEnd
    } else if message.contains("ambiguous context") {
        TemplateErrorCode::ErrAmbigContext
    } else if message.contains("on range loop re-entry") {
        TemplateErrorCode::ErrRangeLoopReentry
    } else if message.contains("cannot compute output context for") {
        TemplateErrorCode::ErrOutputContext
    } else if message.contains("ends in a non-text context") {
        TemplateErrorCode::ErrEndContext
    } else if message.contains("unfinished JS regexp charset") {
        TemplateErrorCode::ErrPartialCharset
    } else if message.contains("unfinished escape sequence") {
        TemplateErrorCode::ErrPartialEscape
    } else if message.contains("could start a division or regexp") {
        TemplateErrorCode::ErrSlashAmbig
    } else if message.contains("predefined escaper") {
        TemplateErrorCode::ErrPredefinedEscaper
    } else if message.contains("expected space, attr name, or end of tag")
        || message.contains("in attribute name")
        || message.contains("in unquoted attr")
    {
        TemplateErrorCode::ErrBadHTML
    } else if message.contains("not valid UTF-8")
        || message.contains("invalid UTF-8 boundary while trimming")
    {
        TemplateErrorCode::ErrInvalidUTF8
    } else if message.contains("no such template") {
        TemplateErrorCode::ErrNoSuchTemplate
    } else {
        TemplateErrorCode::ErrParse
    }
}

fn parse_error_line(message: &str) -> Option<usize> {
    let marker = "line ";
    let start = message.find(marker)?;
    let remainder = &message[start + marker.len()..];
    let digits = remainder
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<usize>().ok()
}

fn parse_error_name(message: &str) -> Option<String> {
    let start = message.find("template `")?;
    let remainder = &message[start + "template `".len()..];
    let end = remainder.find('`')?;
    Some(remainder[..end].to_string())
}

fn render_error_code(message: &str) -> TemplateErrorCode {
    if message.contains("template `") && message.contains("` is not defined") {
        TemplateErrorCode::ErrNotDefined
    } else if message.contains("map has no entry")
        || message.contains("has no entry for key")
        || message.contains("could not be resolved")
    {
        TemplateErrorCode::ErrMissingKey
    } else {
        TemplateErrorCode::ErrRender
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Json(JsonValue),
    SafeHtml(String),
    SafeHtmlAttr(String),
    SafeJs(String),
    SafeCss(String),
    SafeUrl(String),
    SafeSrcset(String),
    FunctionRef(String),
    Missing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HTML(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HTMLAttr(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JS(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JSStr(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CSS(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct URL(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Srcset(pub String);

impl Value {
    pub fn safe_html<S: Into<String>>(value: S) -> Self {
        Self::SafeHtml(value.into())
    }

    pub fn safe_html_attr<S: Into<String>>(value: S) -> Self {
        Self::SafeHtmlAttr(value.into())
    }

    pub fn safe_js<S: Into<String>>(value: S) -> Self {
        Self::SafeJs(value.into())
    }

    pub fn safe_css<S: Into<String>>(value: S) -> Self {
        Self::SafeCss(value.into())
    }

    pub fn safe_url<S: Into<String>>(value: S) -> Self {
        Self::SafeUrl(value.into())
    }

    pub fn safe_srcset<S: Into<String>>(value: S) -> Self {
        Self::SafeSrcset(value.into())
    }

    fn from_serializable<T: Serialize>(data: &T) -> Result<Self> {
        Ok(Self::Json(serde_json::to_value(data)?))
    }

    fn truthy(&self) -> bool {
        match self {
            Value::SafeHtml(value) => !value.is_empty(),
            Value::SafeHtmlAttr(value) => !value.is_empty(),
            Value::SafeJs(value) => !value.is_empty(),
            Value::SafeCss(value) => !value.is_empty(),
            Value::SafeUrl(value) => !value.is_empty(),
            Value::SafeSrcset(value) => !value.is_empty(),
            Value::FunctionRef(_) => true,
            Value::Missing => false,
            Value::Json(value) => match value {
                JsonValue::Null => false,
                JsonValue::Bool(v) => *v,
                JsonValue::Number(v) => {
                    if let Some(i) = v.as_i64() {
                        i != 0
                    } else if let Some(u) = v.as_u64() {
                        u != 0
                    } else if let Some(f) = v.as_f64() {
                        f != 0.0
                    } else {
                        true
                    }
                }
                JsonValue::String(v) => !v.is_empty(),
                JsonValue::Array(v) => !v.is_empty(),
                JsonValue::Object(v) => !v.is_empty(),
            },
        }
    }

    pub fn to_plain_string(&self) -> String {
        match self {
            Value::SafeHtml(value) => value.clone(),
            Value::SafeHtmlAttr(value) => value.clone(),
            Value::SafeJs(value) => value.clone(),
            Value::SafeCss(value) => value.clone(),
            Value::SafeUrl(value) => value.clone(),
            Value::SafeSrcset(value) => value.clone(),
            Value::FunctionRef(name) => format!("<function:{name}>"),
            Value::Missing => "<no value>".to_string(),
            Value::Json(value) => match value {
                JsonValue::Null => String::new(),
                JsonValue::Bool(v) => v.to_string(),
                JsonValue::Number(v) => v.to_string(),
                JsonValue::String(v) => v.clone(),
                JsonValue::Array(_) | JsonValue::Object(_) => value.to_string(),
            },
        }
    }

    fn iter_pairs(&self) -> Vec<(Value, Value)> {
        match self {
            Value::Json(JsonValue::Array(items)) => items
                .iter()
                .enumerate()
                .map(|(index, value)| (Value::from(index as u64), Value::Json(value.clone())))
                .collect::<Vec<_>>(),
            Value::Json(JsonValue::Object(items)) => {
                let mut keys = items.keys().cloned().collect::<Vec<_>>();
                keys.sort();
                keys.into_iter()
                    .map(|key| {
                        let value = items.get(&key).cloned().unwrap_or(JsonValue::Null);
                        (Value::from(key.as_str()), Value::Json(value))
                    })
                    .collect::<Vec<_>>()
            }
            Value::Json(JsonValue::String(value)) => value
                .chars()
                .enumerate()
                .map(|(index, ch)| {
                    (
                        Value::from(index as u64),
                        Value::Json(JsonValue::String(ch.to_string())),
                    )
                })
                .collect::<Vec<_>>(),
            Value::FunctionRef(_)
            | Value::Missing
            | Value::SafeHtml(_)
            | Value::SafeHtmlAttr(_)
            | Value::SafeJs(_)
            | Value::SafeCss(_)
            | Value::SafeUrl(_)
            | Value::SafeSrcset(_)
            | Value::Json(_) => Vec::new(),
        }
    }
}

impl From<HTML> for Value {
    fn from(value: HTML) -> Self {
        Value::SafeHtml(value.0)
    }
}

impl From<HTMLAttr> for Value {
    fn from(value: HTMLAttr) -> Self {
        Value::SafeHtmlAttr(value.0)
    }
}

impl From<JS> for Value {
    fn from(value: JS) -> Self {
        Value::SafeJs(value.0)
    }
}

impl From<JSStr> for Value {
    fn from(value: JSStr) -> Self {
        Value::SafeJs(value.0)
    }
}

impl From<CSS> for Value {
    fn from(value: CSS) -> Self {
        Value::SafeCss(value.0)
    }
}

impl From<URL> for Value {
    fn from(value: URL) -> Self {
        Value::SafeUrl(value.0)
    }
}

impl From<Srcset> for Value {
    fn from(value: Srcset) -> Self {
        Value::SafeSrcset(value.0)
    }
}

impl From<JsonValue> for Value {
    fn from(value: JsonValue) -> Self {
        Value::Json(value)
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::Json(JsonValue::String(value))
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::Json(JsonValue::String(value.to_string()))
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Json(JsonValue::Bool(value))
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Value::Json(JsonValue::Number(value.into()))
    }
}

impl From<u64> for Value {
    fn from(value: u64) -> Self {
        Value::Json(JsonValue::Number(value.into()))
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        match serde_json::Number::from_f64(value) {
            Some(number) => Value::Json(JsonValue::Number(number)),
            None => Value::Json(JsonValue::Null),
        }
    }
}

pub type Function = Arc<dyn Fn(&[Value]) -> Result<Value> + Send + Sync + 'static>;
pub type FuncMap = HashMap<String, Function>;
pub type Method = Arc<dyn Fn(&Value, &[Value]) -> Result<Value> + Send + Sync + 'static>;
pub type MethodMap = HashMap<String, Method>;
type ScopeStack = Vec<HashMap<String, Value>>;

#[derive(Clone, Debug)]
pub struct ParseTree {
    nodes: Vec<Node>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderFlow {
    Normal,
    Break,
    Continue,
}

const MAX_TEMPLATE_EXECUTION_DEPTH: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingKeyMode {
    Default,
    Zero,
    Error,
}

#[derive(Clone)]
struct TemplateNameSpace {
    templates: Arc<RwLock<HashMap<String, Vec<Node>>>>,
    funcs: Arc<RwLock<FuncMap>>,
    methods: Arc<RwLock<MethodMap>>,
    missing_key_mode: Arc<RwLock<MissingKeyMode>>,
    left_delim: Arc<RwLock<String>>,
    right_delim: Arc<RwLock<String>>,
    executed: Arc<AtomicBool>,
}

impl TemplateNameSpace {
    fn new() -> Self {
        Self {
            templates: Arc::new(RwLock::new(HashMap::new())),
            funcs: Arc::new(RwLock::new(builtin_funcs())),
            methods: Arc::new(RwLock::new(HashMap::new())),
            missing_key_mode: Arc::new(RwLock::new(MissingKeyMode::Default)),
            left_delim: Arc::new(RwLock::new("{{".to_string())),
            right_delim: Arc::new(RwLock::new("}}".to_string())),
            executed: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Clone)]
pub struct Template {
    name: String,
    name_space: TemplateNameSpace,
}

impl Template {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            name_space: TemplateNameSpace::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn funcs(self, funcs: FuncMap) -> Self {
        for name in funcs.keys() {
            assert_valid_callable_name(name, "function");
        }
        self.name_space.funcs.write().unwrap().extend(funcs);
        self
    }

    pub fn add_func<F>(self, name: impl Into<String>, function: F) -> Self
    where
        F: Fn(&[Value]) -> Result<Value> + Send + Sync + 'static,
    {
        let name = name.into();
        assert_valid_callable_name(&name, "function");
        self.name_space
            .funcs
            .write()
            .unwrap()
            .insert(name, Arc::new(function));
        self
    }

    pub fn methods(self, methods: MethodMap) -> Self {
        for name in methods.keys() {
            assert_valid_callable_name(name, "method");
        }
        self.name_space.methods.write().unwrap().extend(methods);
        self
    }

    pub fn add_method<F>(self, name: impl Into<String>, method: F) -> Self
    where
        F: Fn(&Value, &[Value]) -> Result<Value> + Send + Sync + 'static,
    {
        let name = name.into();
        assert_valid_callable_name(&name, "method");
        self.name_space
            .methods
            .write()
            .unwrap()
            .insert(name, Arc::new(method));
        self
    }

    pub fn delims(self, left: impl Into<String>, right: impl Into<String>) -> Self {
        let mut left = left.into();
        let mut right = right.into();
        if left.is_empty() {
            left = "{{".to_string();
        }
        if right.is_empty() {
            right = "}}".to_string();
        }
        {
            let mut delimiter = self.name_space.left_delim.write().unwrap();
            *delimiter = left;
        }
        {
            let mut delimiter = self.name_space.right_delim.write().unwrap();
            *delimiter = right;
        }
        self
    }

    pub fn clone_template(&self) -> Result<Self> {
        self.Clone()
    }

    #[allow(non_snake_case)]
    pub fn Clone(&self) -> Result<Self> {
        let left_delim = self.name_space.left_delim.read().unwrap().clone();
        let right_delim = self.name_space.right_delim.read().unwrap().clone();
        if left_delim.is_empty() || right_delim.is_empty() {
            return Err(TemplateError::Parse(
                "template delimiters must not be empty".to_string(),
            ));
        }
        self.ensure_not_executed()?;
        let templates = self.name_space.templates.read().unwrap().clone();
        let funcs = self.name_space.funcs.read().unwrap().clone();
        let methods = self.name_space.methods.read().unwrap().clone();
        let missing_key_mode = self.name_space.missing_key_mode.read().unwrap().clone();

        Ok(Self {
            name: self.name.clone(),
            name_space: TemplateNameSpace {
                templates: Arc::new(RwLock::new(templates)),
                funcs: Arc::new(RwLock::new(funcs)),
                methods: Arc::new(RwLock::new(methods)),
                missing_key_mode: Arc::new(RwLock::new(missing_key_mode)),
                left_delim: Arc::new(RwLock::new(left_delim)),
                right_delim: Arc::new(RwLock::new(right_delim)),
                executed: Arc::new(AtomicBool::new(false)),
            },
        })
    }

    pub fn option(mut self, option: &str) -> Result<Self> {
        self.apply_option(option)?;
        Ok(self)
    }

    pub fn options<I, S>(mut self, options: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for option in options {
            self.apply_option(option.as_ref())?;
        }
        Ok(self)
    }

    fn apply_option(&mut self, option: &str) -> Result<()> {
        let trimmed = option.trim();
        let Some(value) = trimmed.strip_prefix("missingkey=") else {
            return Err(TemplateError::Parse(format!(
                "unsupported option `{trimmed}`"
            )));
        };

        let mode = match value {
            "default" | "invalid" => MissingKeyMode::Default,
            "zero" => MissingKeyMode::Zero,
            "error" => MissingKeyMode::Error,
            _ => {
                return Err(TemplateError::Parse(format!(
                    "unsupported missingkey option `{value}`"
                )));
            }
        };
        *self.name_space.missing_key_mode.write().unwrap() = mode;
        Ok(())
    }

    pub fn parse(mut self, text: &str) -> Result<Self> {
        self.ensure_not_executed()?;
        let root = self.name.clone();
        self.parse_named(&root, text)?;
        self.reanalyze_contexts()?;
        Ok(self)
    }

    #[allow(non_snake_case)]
    pub fn New(&self, name: impl Into<String>) -> Self {
        let mut clone = self.clone();
        clone.name = name.into();
        clone
    }

    pub fn parse_tree(&self, text: &str) -> Result<ParseTree> {
        let preprocessed = strip_html_comments(text);
        let left_delim = self.name_space.left_delim.read().unwrap().clone();
        let right_delim = self.name_space.right_delim.read().unwrap().clone();
        let tokens = tokenize(&preprocessed, &left_delim, &right_delim)?;
        let mut index = 0;
        let (nodes, stop) = parse_nodes(&tokens, &mut index, &[])?;
        if let Some(stop) = stop {
            return Err(TemplateError::Parse(format!(
                "unexpected control action `{}`",
                stop.keyword
            )));
        }
        Ok(ParseTree { nodes })
    }

    #[allow(non_snake_case)]
    pub fn AddParseTree(mut self, name: impl Into<String>, tree: ParseTree) -> Result<Self> {
        self.ensure_not_executed()?;
        let name = name.into();
        self.validate_function_calls(&tree.nodes)?;
        if !self
            .name_space
            .templates
            .read()
            .unwrap()
            .contains_key(&self.name)
        {
            self.name = name.clone();
        }
        {
            let mut templates = self.name_space.templates.write().unwrap();
            self.merge_template_nodes(&mut templates, &name, tree.nodes);
        }
        self.reanalyze_contexts()?;
        Ok(self)
    }

    pub fn add_parse_tree(self, name: impl Into<String>, tree: ParseTree) -> Result<Self> {
        self.AddParseTree(name, tree)
    }

    /// Parse templates from file paths.
    ///
    /// Note: this API is not available in `web-rust` builds.
    #[cfg(not(feature = "web-rust"))]
    pub fn parse_files<I, P>(mut self, paths: I) -> Result<Self>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        self.ensure_not_executed()?;
        let mut parsed_any = false;
        for path in paths {
            let path = path.as_ref();
            let source = fs::read(path)?;
            let source = std::str::from_utf8(&source).map_err(|error| {
                TemplateError::Parse(format!(
                    "template `{}` is not valid UTF-8: {error}",
                    path.display()
                ))
            })?;
            let name = path
                .file_name()
                .and_then(|part| part.to_str())
                .ok_or_else(|| {
                    TemplateError::Parse(format!("invalid template file name: {}", path.display()))
                })?
                .to_string();

            self.parse_named(&name, &source)?;
            if !self
                .name_space
                .templates
                .read()
                .unwrap()
                .contains_key(&self.name)
            {
                self.name = name;
            }
            parsed_any = true;
        }

        if !parsed_any {
            return Err(TemplateError::Parse(
                "parse_files requires at least one path".to_string(),
            ));
        }

        self.reanalyze_contexts()?;
        Ok(self)
    }

    /// Parse templates from file paths.
    ///
    /// Note: this API is not available in `web-rust` builds.
    #[cfg(feature = "web-rust")]
    pub fn parse_files<I, P>(self, _paths: I) -> Result<Self>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        self.ensure_not_executed()?;
        Err(TemplateError::Parse(
            "parse_files is not supported in web-rust builds".to_string(),
        ))
    }

    /// Parse templates from glob pattern.
    ///
    /// Note: this API is not available in `web-rust` builds.
    #[cfg(not(feature = "web-rust"))]
    pub fn parse_glob(self, pattern: &str) -> Result<Self> {
        self.ensure_not_executed()?;
        let mut paths = Vec::new();
        for entry in glob::glob(pattern)? {
            paths.push(entry?);
        }
        paths.sort();
        self.parse_files(paths)
    }

    /// Parse templates from glob pattern.
    ///
    /// Note: this API is not available in `web-rust` builds.
    #[cfg(feature = "web-rust")]
    pub fn parse_glob(self, _pattern: &str) -> Result<Self> {
        self.ensure_not_executed()?;
        Err(TemplateError::Parse(
            "parse_glob is not supported in web-rust builds".to_string(),
        ))
    }

    /// Parse templates from file system pattern list.
    ///
    /// Note: this API is not available in `web-rust` builds.
    #[cfg(not(feature = "web-rust"))]
    pub fn parse_fs<I, S>(self, patterns: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.ensure_not_executed()?;
        self.ParseFS(&OSFileSystem, patterns)
    }

    /// Parse templates from a file-system abstraction.
    #[cfg(not(feature = "web-rust"))]
    #[allow(non_snake_case)]
    pub fn ParseFS<F, I, S>(mut self, fs: &F, patterns: I) -> Result<Self>
    where
        F: TemplateFS,
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.ensure_not_executed()?;
        let paths = glob_patterns_with_fsys(fs, patterns)?;
        parse_files_with_fsys(&mut self, fs, paths)?;
        Ok(self)
    }

    /// Parse templates from file system pattern list.
    ///
    /// Note: this API is not available in `web-rust` builds.
    #[cfg(feature = "web-rust")]
    pub fn parse_fs<I, S>(self, _patterns: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.ensure_not_executed()?;
        Err(TemplateError::Parse(
            "parse_fs is not supported in web-rust builds".to_string(),
        ))
    }

    /// Parse templates from a file-system abstraction.
    #[cfg(feature = "web-rust")]
    #[allow(non_snake_case)]
    pub fn ParseFS<F, I, S>(self, _fs: &F, _patterns: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.ensure_not_executed()?;
        Err(TemplateError::Parse(
            "parse_fs is not supported in web-rust builds".to_string(),
        ))
    }

    pub fn execute<T: Serialize, W: Write>(&self, writer: &mut W, data: &T) -> Result<()> {
        self.execute_template(writer, &self.name, data)
    }

    pub fn execute_to_string<T: Serialize>(&self, data: &T) -> Result<String> {
        self.execute_template_to_string(&self.name, data)
    }

    pub fn execute_template<T: Serialize, W: Write>(
        &self,
        writer: &mut W,
        name: &str,
        data: &T,
    ) -> Result<()> {
        self.name_space.executed.store(true, AtomicOrdering::SeqCst);
        let root = Value::from_serializable(data)?;
        let mut rendered = String::new();
        let mut scopes = vec![HashMap::new()];
        let flow = self.render_named(name, &root, &root, &mut scopes, &mut rendered, false, 0)?;
        if !matches!(flow, RenderFlow::Normal) {
            return Err(TemplateError::Render(
                "break/continue action is not inside range".to_string(),
            ));
        }
        writer.write_all(rendered.as_bytes())?;
        Ok(())
    }

    pub fn execute_template_to_string<T: Serialize>(&self, name: &str, data: &T) -> Result<String> {
        self.name_space.executed.store(true, AtomicOrdering::SeqCst);
        let root = Value::from_serializable(data)?;
        let mut rendered = String::new();
        let mut scopes = vec![HashMap::new()];
        let flow = self.render_named(name, &root, &root, &mut scopes, &mut rendered, false, 0)?;
        if !matches!(flow, RenderFlow::Normal) {
            return Err(TemplateError::Render(
                "break/continue action is not inside range".to_string(),
            ));
        }
        Ok(rendered)
    }

    pub fn lookup(&self, name: &str) -> Option<Self> {
        if self.name_space.templates.read().unwrap().contains_key(name) {
            let mut clone = self.clone();
            clone.name = name.to_string();
            Some(clone)
        } else {
            None
        }
    }

    pub fn has_template(&self, name: &str) -> bool {
        self.name_space.templates.read().unwrap().contains_key(name)
    }

    #[allow(non_snake_case)]
    pub fn Templates(&self) -> Vec<Self> {
        self.templates()
    }

    pub fn defined_templates(&self) -> Vec<String> {
        let templates = self.name_space.templates.read().unwrap();
        let mut names = templates.keys().cloned().collect::<Vec<_>>();
        names.sort();
        names
    }

    pub fn defined_templates_string(&self) -> String {
        let names = self.defined_templates();
        if names.is_empty() {
            String::new()
        } else {
            format!("; defined templates are: {}", names.join(", "))
        }
    }

    #[allow(non_snake_case)]
    pub fn DefinedTemplates(&self) -> String {
        self.defined_templates_string()
    }

    pub fn templates(&self) -> Vec<Self> {
        let names = self.defined_templates();
        names
            .into_iter()
            .filter_map(|name| self.lookup(&name))
            .collect::<Vec<_>>()
    }

    fn parse_named(&mut self, name: &str, text: &str) -> Result<()> {
        validate_template_hazards(text)?;
        let tree = self.parse_tree(text)?;
        self.validate_function_calls(&tree.nodes)?;
        let mut templates = self.name_space.templates.write().unwrap();
        self.merge_template_nodes(&mut templates, name, tree.nodes);

        Ok(())
    }

    fn merge_template_nodes(
        &self,
        templates: &mut HashMap<String, Vec<Node>>,
        name: &str,
        nodes: Vec<Node>,
    ) {
        let mut root_nodes = Vec::new();
        for node in nodes {
            match node {
                Node::Define {
                    name: defined_name,
                    body,
                } => {
                    if !is_empty_template_body(&body) || !templates.contains_key(&defined_name) {
                        templates.insert(defined_name, body);
                    }
                }
                Node::Block {
                    name: block_name,
                    data,
                    body,
                } => {
                    templates
                        .entry(block_name.clone())
                        .or_insert_with(|| body.clone());
                    root_nodes.push(Node::TemplateCall {
                        name: block_name,
                        data,
                    });
                }
                other => root_nodes.push(other),
            }
        }

        if !root_nodes.is_empty() || !templates.contains_key(name) {
            templates.insert(name.to_string(), root_nodes);
        }
    }

    fn ensure_not_executed(&self) -> Result<()> {
        if self.name_space.executed.load(AtomicOrdering::SeqCst) {
            return Err(TemplateError::Parse(
                "template cannot be parsed or cloned after execution".to_string(),
            ));
        }
        Ok(())
    }

    fn validate_function_calls(&self, nodes: &[Node]) -> Result<()> {
        let funcs = self.name_space.funcs.read().unwrap();
        validate_function_calls_in_nodes(nodes, &funcs)
    }

    fn reanalyze_contexts(&mut self) -> Result<()> {
        let raw_templates = self.name_space.templates.read().unwrap().clone();
        if !raw_templates.contains_key(&self.name) {
            return Err(TemplateError::Parse(format!(
                "template `{}` is not defined",
                self.name
            )));
        }

        let mut analyzer = ParseContextAnalyzer::new(raw_templates.clone());
        let root_start = ContextState::html_text();
        let root_end = analyzer.analyze_template(&self.name, root_start)?;
        if !root_end.is_text_context() {
            return Err(TemplateError::Parse(format!(
                "template `{}` ends in a non-text context",
                self.name
            )));
        }

        // Analyze unreferenced templates with HTML start context so
        // execute_template(name, ...) has stable precomputed escaping.
        let mut names = raw_templates.keys().cloned().collect::<Vec<_>>();
        names.sort();
        for name in names.iter() {
            if !analyzer.has_analysis(name.as_str()) {
                let _ = analyzer.analyze_template(name.as_str(), ContextState::html_text())?;
            }
        }

        *self.name_space.templates.write().unwrap() = analyzer.finish();
        Ok(())
    }

    fn render_named(
        &self,
        name: &str,
        root: &Value,
        dot: &Value,
        scopes: &mut ScopeStack,
        output: &mut String,
        in_range: bool,
        depth: usize,
    ) -> Result<RenderFlow> {
        if depth > MAX_TEMPLATE_EXECUTION_DEPTH {
            return Err(TemplateError::Render(format!(
                "template `{name}` exceeds maximum execution depth of {MAX_TEMPLATE_EXECUTION_DEPTH}"
            )));
        }

        let templates = self.name_space.templates.read().unwrap();
        let nodes = templates
            .get(name)
            .ok_or_else(|| TemplateError::Render(format!("template `{name}` is not defined")))?;
        self.render_nodes(nodes, root, dot, scopes, output, in_range, depth)
    }

    fn render_nodes(
        &self,
        nodes: &[Node],
        root: &Value,
        dot: &Value,
        scopes: &mut ScopeStack,
        output: &mut String,
        in_range: bool,
        depth: usize,
    ) -> Result<RenderFlow> {
        for node in nodes {
            match node {
                Node::Text(text) => {
                    let mode = infer_escape_mode(output);
                    if matches!(mode, EscapeMode::ScriptExpr) {
                        output.push_str(&filter_script_text(output, text));
                    } else if matches!(mode, EscapeMode::Html) {
                        output.push_str(&filter_html_text_sections(output, text));
                    } else {
                        output.push_str(text);
                    }
                }
                Node::Expr { expr, mode } => {
                    let mut mode = *mode;
                    if matches!(
                        mode,
                        EscapeMode::AttrQuoted {
                            kind: AttrKind::Normal,
                            ..
                        } | EscapeMode::AttrUnquoted {
                            kind: AttrKind::Normal
                        }
                    ) {
                        let inferred_mode = infer_escape_mode(output);
                        if !matches!(inferred_mode, EscapeMode::AttrName) {
                            mode = inferred_mode;
                        }
                    }
                    let value = self.eval_expr(expr, root, dot, scopes)?;
                    output.push_str(&escape_value_for_mode(&value, mode)?);
                }
                Node::SetVar {
                    name,
                    value,
                    declare,
                } => {
                    let evaluated = self.eval_expr(value, root, dot, scopes)?;
                    if *declare {
                        declare_variable(scopes, name, evaluated);
                    } else {
                        assign_variable(scopes, name, evaluated)?;
                    }
                }
                Node::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    let condition_value = self.eval_expr(condition, root, dot, scopes)?;
                    if condition_value.truthy() {
                        push_scope(scopes);
                        let flow = self.render_nodes(
                            then_branch,
                            root,
                            dot,
                            scopes,
                            output,
                            in_range,
                            depth,
                        )?;
                        pop_scope(scopes);
                        if !matches!(flow, RenderFlow::Normal) {
                            return Ok(flow);
                        }
                    } else {
                        push_scope(scopes);
                        let flow = self.render_nodes(
                            else_branch,
                            root,
                            dot,
                            scopes,
                            output,
                            in_range,
                            depth,
                        )?;
                        pop_scope(scopes);
                        if !matches!(flow, RenderFlow::Normal) {
                            return Ok(flow);
                        }
                    }
                }
                Node::Range {
                    vars,
                    declare_vars,
                    iterable,
                    body,
                    else_branch,
                } => {
                    let iterable_value = self.eval_expr(iterable, root, dot, scopes)?;
                    let items = iterable_value.iter_pairs();
                    if items.is_empty() {
                        push_scope(scopes);
                        let flow =
                            self.render_nodes(else_branch, root, dot, scopes, output, true, depth)?;
                        pop_scope(scopes);
                        match flow {
                            RenderFlow::Normal | RenderFlow::Break | RenderFlow::Continue => {}
                        }
                    } else {
                        for (key, item) in items {
                            push_scope(scopes);
                            if vars.len() == 1 {
                                if *declare_vars {
                                    declare_variable(scopes, &vars[0], item.clone());
                                } else {
                                    assign_variable(scopes, &vars[0], item.clone())?;
                                }
                            } else if vars.len() == 2 {
                                if *declare_vars {
                                    declare_variable(scopes, &vars[0], key);
                                    declare_variable(scopes, &vars[1], item.clone());
                                } else {
                                    assign_variable(scopes, &vars[0], key)?;
                                    assign_variable(scopes, &vars[1], item.clone())?;
                                }
                            }
                            let flow =
                                self.render_nodes(body, root, &item, scopes, output, true, depth)?;
                            pop_scope(scopes);
                            match flow {
                                RenderFlow::Normal => {}
                                RenderFlow::Continue => continue,
                                RenderFlow::Break => break,
                            }
                        }
                    }
                }
                Node::With {
                    value,
                    body,
                    else_branch,
                } => {
                    let value = self.eval_expr(value, root, dot, scopes)?;
                    if value.truthy() {
                        push_scope(scopes);
                        let flow =
                            self.render_nodes(body, root, &value, scopes, output, in_range, depth)?;
                        pop_scope(scopes);
                        if !matches!(flow, RenderFlow::Normal) {
                            return Ok(flow);
                        }
                    } else {
                        push_scope(scopes);
                        let flow = self.render_nodes(
                            else_branch,
                            root,
                            dot,
                            scopes,
                            output,
                            in_range,
                            depth,
                        )?;
                        pop_scope(scopes);
                        if !matches!(flow, RenderFlow::Normal) {
                            return Ok(flow);
                        }
                    }
                }
                Node::TemplateCall { name, data } => {
                    let next_dot = match data {
                        Some(expr) => self.eval_expr(expr, root, dot, scopes)?,
                        None => dot.clone(),
                    };
                    let next_depth = depth + 1;
                    if next_depth > MAX_TEMPLATE_EXECUTION_DEPTH {
                        return Err(TemplateError::Render(format!(
                            "template `{name}` exceeds maximum execution depth of {MAX_TEMPLATE_EXECUTION_DEPTH}"
                        )));
                    }
                    let mut template_scopes = vec![HashMap::new()];
                    let flow = self.render_named(
                        name,
                        root,
                        &next_dot,
                        &mut template_scopes,
                        output,
                        in_range,
                        next_depth,
                    )?;
                    if !matches!(flow, RenderFlow::Normal) {
                        return Ok(flow);
                    }
                }
                Node::Block { name, data, body } => {
                    let next_dot = match data {
                        Some(expr) => self.eval_expr(expr, root, dot, scopes)?,
                        None => dot.clone(),
                    };

                    if self.name_space.templates.read().unwrap().contains_key(name) {
                        let next_depth = depth + 1;
                        if next_depth > MAX_TEMPLATE_EXECUTION_DEPTH {
                            return Err(TemplateError::Render(format!(
                                "template `{name}` exceeds maximum execution depth of {MAX_TEMPLATE_EXECUTION_DEPTH}"
                            )));
                        }
                        let mut template_scopes = vec![HashMap::new()];
                        let flow = self.render_named(
                            name,
                            root,
                            &next_dot,
                            &mut template_scopes,
                            output,
                            in_range,
                            next_depth,
                        )?;
                        if !matches!(flow, RenderFlow::Normal) {
                            return Ok(flow);
                        }
                    } else {
                        push_scope(scopes);
                        let flow = self
                            .render_nodes(body, root, &next_dot, scopes, output, in_range, depth)?;
                        pop_scope(scopes);
                        if !matches!(flow, RenderFlow::Normal) {
                            return Ok(flow);
                        }
                    }
                }
                Node::Define { .. } => {}
                Node::Break => {
                    if in_range {
                        return Ok(RenderFlow::Break);
                    }
                    return Err(TemplateError::Render(
                        "break action is not inside range".to_string(),
                    ));
                }
                Node::Continue => {
                    if in_range {
                        return Ok(RenderFlow::Continue);
                    }
                    return Err(TemplateError::Render(
                        "continue action is not inside range".to_string(),
                    ));
                }
            }
        }

        Ok(RenderFlow::Normal)
    }

    fn eval_expr(
        &self,
        expr: &Expr,
        root: &Value,
        dot: &Value,
        scopes: &ScopeStack,
    ) -> Result<Value> {
        let mut piped: Option<Value> = None;

        for (index, command) in expr.commands.iter().enumerate() {
            match command {
                Command::Value(term) => {
                    if index > 0 {
                        return Err(TemplateError::Render(
                            "pipeline command must be a function".to_string(),
                        ));
                    }
                    piped = Some(self.eval_term(term, root, dot, scopes)?);
                }
                Command::Call { name, args } => {
                    let mut evaluated_args = args
                        .iter()
                        .map(|arg| self.eval_term(arg, root, dot, scopes))
                        .collect::<Result<Vec<_>>>()?;

                    if index > 0 {
                        let value = piped.take().ok_or_else(|| {
                            TemplateError::Render("pipeline is missing input value".to_string())
                        })?;
                        evaluated_args.push(value);
                    }

                    if index == 0 && evaluated_args.is_empty() {
                        let methods = self.name_space.methods.read().unwrap();
                        let missing_key_mode = *self.name_space.missing_key_mode.read().unwrap();
                        if let Some(value) =
                            lookup_identifier(dot, root, name, &methods, missing_key_mode)?
                        {
                            piped = Some(value);
                            continue;
                        }

                        let funcs = self.name_space.funcs.read().unwrap();
                        if !funcs.contains_key(name) {
                            if matches!(dot, Value::Json(JsonValue::Object(_)))
                                || matches!(root, Value::Json(JsonValue::Object(_)))
                            {
                                piped = Some(missing_value_for_key(name, missing_key_mode)?);
                                continue;
                            }
                        }
                    }

                    if name == "call" {
                        piped = Some(self.eval_call_function(&evaluated_args)?);
                    } else {
                        let funcs = self.name_space.funcs.read().unwrap();
                        let function = funcs.get(name).ok_or_else(|| {
                            TemplateError::Render(format!("function `{name}` is not registered"))
                        })?;
                        piped = Some(function(&evaluated_args)?);
                    }
                }
                Command::Invoke { callee, args } => {
                    let mut evaluated_args = args
                        .iter()
                        .map(|arg| self.eval_term(arg, root, dot, scopes))
                        .collect::<Result<Vec<_>>>()?;

                    if index > 0 {
                        let value = piped.take().ok_or_else(|| {
                            TemplateError::Render("pipeline is missing input value".to_string())
                        })?;
                        evaluated_args.push(value);
                    }
                    piped =
                        Some(self.eval_method_call(callee, &evaluated_args, root, dot, scopes)?);
                }
            }
        }

        piped.ok_or_else(|| TemplateError::Render("empty expression".to_string()))
    }

    fn eval_term(
        &self,
        term: &Term,
        root: &Value,
        dot: &Value,
        scopes: &ScopeStack,
    ) -> Result<Value> {
        match term {
            Term::DotPath(path) => {
                let methods = self.name_space.methods.read().unwrap();
                let missing_key_mode = *self.name_space.missing_key_mode.read().unwrap();
                lookup_path_with_methods(dot, path, &methods, missing_key_mode)
            }
            Term::RootPath(path) => {
                let methods = self.name_space.methods.read().unwrap();
                let missing_key_mode = *self.name_space.missing_key_mode.read().unwrap();
                lookup_path_with_methods(root, path, &methods, missing_key_mode)
            }
            Term::Literal(value) => Ok(value.clone()),
            Term::Variable { name, path } => {
                let variable = lookup_variable(scopes, name).ok_or_else(|| {
                    TemplateError::Render(format!("variable `${name}` could not be resolved"))
                })?;
                let methods = self.name_space.methods.read().unwrap();
                let missing_key_mode = *self.name_space.missing_key_mode.read().unwrap();
                lookup_path_with_methods(&variable, path, &methods, missing_key_mode)
            }
            Term::Identifier(name) => {
                let methods = self.name_space.methods.read().unwrap();
                let missing_key_mode = *self.name_space.missing_key_mode.read().unwrap();
                if let Some(value) = lookup_identifier(dot, root, name, &methods, missing_key_mode)?
                {
                    Ok(value)
                } else if self.name_space.funcs.read().unwrap().contains_key(name) {
                    Ok(Value::FunctionRef(name.clone()))
                } else if matches!(dot, Value::Json(JsonValue::Object(_)))
                    || matches!(root, Value::Json(JsonValue::Object(_)))
                {
                    missing_value_for_key(name, missing_key_mode)
                } else {
                    Err(TemplateError::Render(format!(
                        "identifier `{name}` could not be resolved"
                    )))
                }
            }
            Term::SubExpr(expr) => self.eval_expr(expr, root, dot, scopes),
            Term::SubExprPath { expr, path } => {
                let base = self.eval_expr(expr, root, dot, scopes)?;
                let methods = self.name_space.methods.read().unwrap();
                let missing_key_mode = *self.name_space.missing_key_mode.read().unwrap();
                lookup_path_with_methods(&base, path, &methods, missing_key_mode)
            }
        }
    }

    fn eval_method_call(
        &self,
        callee: &Term,
        args: &[Value],
        root: &Value,
        dot: &Value,
        scopes: &ScopeStack,
    ) -> Result<Value> {
        match callee {
            Term::DotPath(path) => self.call_path_method(dot, path, args),
            Term::RootPath(path) => self.call_path_method(root, path, args),
            Term::Variable { name, path } => {
                let variable = lookup_variable(scopes, name).ok_or_else(|| {
                    TemplateError::Render(format!("variable `${name}` could not be resolved"))
                })?;
                self.call_path_method(&variable, path, args)
            }
            Term::Identifier(name) => {
                let methods = self.name_space.methods.read().unwrap();
                if let Some(method) = methods.get(name) {
                    return method(dot, args);
                }
                Err(TemplateError::Render(format!(
                    "callee `{name}` is not a callable method"
                )))
            }
            Term::Literal(_) => Err(TemplateError::Render(
                "literal values are not callable".to_string(),
            )),
            Term::SubExpr(_) => Err(TemplateError::Render(
                "parenthesized expressions are not callable".to_string(),
            )),
            Term::SubExprPath { expr, path } => {
                let receiver = self.eval_expr(expr, root, dot, scopes)?;
                self.call_path_method(&receiver, path, args)
            }
        }
    }

    fn call_path_method(&self, base: &Value, path: &[String], args: &[Value]) -> Result<Value> {
        if path.is_empty() {
            return Err(TemplateError::Render("path is not callable".to_string()));
        }

        let (method_name, receiver_path) = split_last_path(path);
        let methods = self.name_space.methods.read().unwrap();
        let missing_key_mode = *self.name_space.missing_key_mode.read().unwrap();
        let receiver = lookup_path_with_methods(base, receiver_path, &methods, missing_key_mode)?;
        let method = methods.get(method_name).ok_or_else(|| {
            TemplateError::Render(format!("method `{method_name}` is not registered"))
        })?;
        method(&receiver, args)
    }

    fn eval_call_function(&self, args: &[Value]) -> Result<Value> {
        if args.is_empty() {
            return Err(TemplateError::Render(
                "call expects at least one argument".to_string(),
            ));
        }

        let name = match &args[0] {
            Value::FunctionRef(name) => name.clone(),
            Value::Json(JsonValue::String(name)) => name.clone(),
            other => {
                return Err(TemplateError::Render(format!(
                    "call expects function reference or function name, got `{}`",
                    other.to_plain_string()
                )));
            }
        };

        let function = {
            let funcs = self.name_space.funcs.read().unwrap();
            funcs.get(&name).cloned().ok_or_else(|| {
                TemplateError::Render(format!("function `{name}` is not registered"))
            })?
        };
        function(&args[1..])
    }
}

pub fn must(result: Result<Template>) -> Template {
    match result {
        Ok(template) => template,
        Err(error) => panic!("{error}"),
    }
}

/// Parse templates from file paths.
///
/// Note: this API is not available in `web-rust` builds.
#[cfg(not(feature = "web-rust"))]
pub fn parse_files<I, P>(paths: I) -> Result<Template>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let paths = paths
        .into_iter()
        .map(|path| path.as_ref().to_path_buf())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Err(TemplateError::Parse(
            "parse_files requires at least one path".to_string(),
        ));
    }

    let name = template_name_from_path(&paths[0])?;
    Template::new(name).parse_files(paths)
}

/// Parse templates from file paths.
///
/// Note: this API is not available in `web-rust` builds.
#[cfg(feature = "web-rust")]
pub fn parse_files<I, P>(_: I) -> Result<Template>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    Err(TemplateError::Parse(
        "parse_files is not supported in web-rust builds".to_string(),
    ))
}

/// Parse templates from glob pattern.
///
/// Note: this API is not available in `web-rust` builds.
#[cfg(not(feature = "web-rust"))]
pub fn parse_glob(pattern: &str) -> Result<Template> {
    let paths = expand_glob_patterns([pattern])?;
    let name = template_name_from_path(&paths[0])?;
    Template::new(name).parse_files(paths)
}

/// Parse templates from glob pattern.
///
/// Note: this API is not available in `web-rust` builds.
#[cfg(feature = "web-rust")]
pub fn parse_glob(_pattern: &str) -> Result<Template> {
    Err(TemplateError::Parse(
        "parse_glob is not supported in web-rust builds".to_string(),
    ))
}

/// Parse templates from file system pattern list.
///
/// Note: this API is not available in `web-rust` builds.
#[cfg(not(feature = "web-rust"))]
pub fn parse_fs<I, S>(patterns: I) -> Result<Template>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    Template::new("").parse_fs(patterns)
}

/// Parse templates from a file-system abstraction.
#[cfg(not(feature = "web-rust"))]
#[allow(non_snake_case)]
pub fn ParseFS<F, I, S>(fs: &F, patterns: I) -> Result<Template>
where
    F: TemplateFS,
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    Template::new("").ParseFS(fs, patterns)
}

/// Parse templates from file system pattern list.
///
/// Note: this API is not available in `web-rust` builds.
#[cfg(feature = "web-rust")]
pub fn parse_fs<I, S>(_: I) -> Result<Template>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    Err(TemplateError::Parse(
        "parse_fs is not supported in web-rust builds".to_string(),
    ))
}

#[cfg(feature = "web-rust")]
#[allow(non_snake_case)]
pub fn ParseFS<F, I, S>(_: &F, _: I) -> Result<Template>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    Err(TemplateError::Parse(
        "parse_fs is not supported in web-rust builds".to_string(),
    ))
}

#[cfg(not(feature = "web-rust"))]
fn template_name_from_path(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|part| part.to_str())
        .map(str::to_string)
        .ok_or_else(|| {
            TemplateError::Parse(format!("invalid template file name: {}", path.display()))
        })
}

pub fn is_true(value: &Value) -> (bool, bool) {
    if matches!(value, Value::Missing) {
        return (false, false);
    }
    (value.truthy(), true)
}

pub fn html_escape<W: Write>(writer: &mut W, bytes: &[u8]) -> std::io::Result<()> {
    writer.write_all(escape_html(&String::from_utf8_lossy(bytes)).as_bytes())
}

pub fn html_escape_string(value: &str) -> String {
    escape_html(value)
}

pub fn html_escaper(args: &[Value]) -> String {
    let mut combined = String::new();
    for arg in args {
        combined.push_str(&arg.to_plain_string());
    }
    escape_html(&combined)
}

pub fn js_escape<W: Write>(writer: &mut W, bytes: &[u8]) -> std::io::Result<()> {
    writer.write_all(js_escape_string(&String::from_utf8_lossy(bytes)).as_bytes())
}

pub fn js_escape_string(value: &str) -> String {
    escape_js_string_fragment(value, '"')
}

pub fn js_escaper(args: &[Value]) -> String {
    let mut combined = String::new();
    for arg in args {
        combined.push_str(&arg.to_plain_string());
    }
    js_escape_string(&combined)
}

pub fn url_query_escaper(args: &[Value]) -> String {
    let mut combined = String::new();
    for arg in args {
        combined.push_str(&arg.to_plain_string());
    }
    percent_encode_url(&combined)
}

#[allow(non_snake_case)]
pub fn HTMLEscape<W: Write>(writer: &mut W, bytes: &[u8]) -> std::io::Result<()> {
    html_escape(writer, bytes)
}

#[allow(non_snake_case)]
pub fn HTMLEscapeString(value: &str) -> String {
    html_escape_string(value)
}

#[allow(non_snake_case)]
pub fn HTMLEscaper(args: &[Value]) -> String {
    html_escaper(args)
}

#[allow(non_snake_case)]
pub fn JSEscape<W: Write>(writer: &mut W, bytes: &[u8]) -> std::io::Result<()> {
    js_escape(writer, bytes)
}

#[allow(non_snake_case)]
pub fn JSEscapeString(value: &str) -> String {
    js_escape_string(value)
}

#[allow(non_snake_case)]
pub fn JSEscaper(args: &[Value]) -> String {
    js_escaper(args)
}

#[allow(non_snake_case)]
pub fn URLQueryEscaper(args: &[Value]) -> String {
    url_query_escaper(args)
}

#[allow(non_snake_case)]
pub fn IsTrue(value: &Value) -> (bool, bool) {
    is_true(value)
}

#[derive(Clone, Debug)]
enum Node {
    Text(String),
    Expr {
        expr: Expr,
        mode: EscapeMode,
    },
    SetVar {
        name: String,
        value: Expr,
        declare: bool,
    },
    If {
        condition: Expr,
        then_branch: Vec<Node>,
        else_branch: Vec<Node>,
    },
    Range {
        vars: Vec<String>,
        declare_vars: bool,
        iterable: Expr,
        body: Vec<Node>,
        else_branch: Vec<Node>,
    },
    With {
        value: Expr,
        body: Vec<Node>,
        else_branch: Vec<Node>,
    },
    TemplateCall {
        name: String,
        data: Option<Expr>,
    },
    Block {
        name: String,
        data: Option<Expr>,
        body: Vec<Node>,
    },
    Define {
        name: String,
        body: Vec<Node>,
    },
    Break,
    Continue,
}

#[derive(Clone, Debug)]
struct Expr {
    commands: Vec<Command>,
}

#[derive(Clone, Debug)]
enum Command {
    Value(Term),
    Call { name: String, args: Vec<Term> },
    Invoke { callee: Term, args: Vec<Term> },
}

#[derive(Clone, Debug)]
enum Term {
    DotPath(Vec<String>),
    RootPath(Vec<String>),
    Variable { name: String, path: Vec<String> },
    Literal(Value),
    Identifier(String),
    SubExpr(Box<Expr>),
    SubExprPath { expr: Box<Expr>, path: Vec<String> },
}

#[derive(Clone, Debug)]
enum Token {
    Text(String),
    Action(String),
}

#[derive(Clone, Debug)]
struct StopAction {
    keyword: String,
    tail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ContextState {
    mode: EscapeMode,
    in_open_tag: bool,
    in_css_attribute: bool,
    css_attribute_quote: Option<char>,
}

impl ContextState {
    fn html_text() -> Self {
        Self {
            mode: EscapeMode::Html,
            in_open_tag: false,
            in_css_attribute: false,
            css_attribute_quote: None,
        }
    }

    fn from_rendered(rendered: &str) -> Self {
        let mode = infer_escape_mode(rendered);
        let tag_value_context = current_tag_value_context(rendered);
        let (in_css_attribute, css_attribute_quote) = match &tag_value_context {
            Some(context) if attr_kind(&context.attr_name) == AttrKind::Css => {
                (true, context.quote)
            }
            _ => (false, None),
        };
        let in_open_tag = (matches!(mode, EscapeMode::Html)
            && is_in_unclosed_tag_context(rendered))
            || matches!(mode, EscapeMode::AttrName);
        Self {
            mode,
            in_open_tag,
            in_css_attribute,
            css_attribute_quote,
        }
    }

    fn is_text_context(&self) -> bool {
        matches!(self.mode, EscapeMode::Html) && !self.in_open_tag
    }
}

#[derive(Clone, Debug)]
struct ContextTracker {
    rendered: String,
}

impl ContextTracker {
    fn from_state(state: ContextState) -> Self {
        Self {
            rendered: seed_rendered_for_state(&state),
        }
    }

    fn state(&self) -> ContextState {
        ContextState::from_rendered(&self.rendered)
    }

    fn mode(&self) -> EscapeMode {
        self.state().mode
    }

    fn append_text(&mut self, text: &str) {
        self.rendered.push_str(text);
        let state = self.state();
        if !matches!(state.mode, EscapeMode::AttrName)
            && !in_css_attribute_context(&self.rendered)
            && !matches!(
                state.mode,
                EscapeMode::ScriptExpr
                    | EscapeMode::ScriptTemplate
                    | EscapeMode::ScriptRegexp
                    | EscapeMode::ScriptLineComment
                    | EscapeMode::ScriptBlockComment
            )
        {
            self.normalize();
        }
    }

    fn append_expr_placeholder(&mut self, mode: EscapeMode) {
        self.rendered.push_str(placeholder_for_mode(mode));
        if !matches!(mode, EscapeMode::AttrName)
            && !in_css_attribute_context(&self.rendered)
            && !matches!(
                mode,
                EscapeMode::ScriptExpr
                    | EscapeMode::ScriptTemplate
                    | EscapeMode::ScriptRegexp
                    | EscapeMode::ScriptLineComment
                    | EscapeMode::ScriptBlockComment
            )
        {
            self.normalize();
        }
    }

    fn normalize(&mut self) {
        let state = self.state();
        self.rendered = seed_rendered_for_state(&state);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum AnalysisFlowKind {
    Normal,
    Break,
    Continue,
}

#[derive(Clone, Debug)]
struct AnalysisFlow {
    kind: AnalysisFlowKind,
    tracker: ContextTracker,
}

impl AnalysisFlow {
    fn normal(tracker: ContextTracker) -> Self {
        Self {
            kind: AnalysisFlowKind::Normal,
            tracker,
        }
    }

    fn with_kind(kind: AnalysisFlowKind, tracker: ContextTracker) -> Self {
        Self { kind, tracker }
    }
}

struct ParseContextAnalyzer {
    raw_templates: HashMap<String, Vec<Node>>,
    analyzed_templates: HashMap<String, Vec<Node>>,
    start_states: HashMap<String, ContextState>,
    end_states: HashMap<String, ContextState>,
    in_progress: HashSet<String>,
    recursive_templates: HashSet<String>,
}

impl ParseContextAnalyzer {
    fn new(raw_templates: HashMap<String, Vec<Node>>) -> Self {
        Self {
            raw_templates,
            analyzed_templates: HashMap::new(),
            start_states: HashMap::new(),
            end_states: HashMap::new(),
            in_progress: HashSet::new(),
            recursive_templates: HashSet::new(),
        }
    }

    fn has_analysis(&self, name: &str) -> bool {
        self.analyzed_templates.contains_key(name)
    }

    fn finish(self) -> HashMap<String, Vec<Node>> {
        self.analyzed_templates
    }

    fn analyze_template(&mut self, name: &str, start_state: ContextState) -> Result<ContextState> {
        if self.start_states.contains_key(name) {
            if let Some(end) = self.end_states.get(name) {
                return Ok(end.clone());
            }

            if self.in_progress.contains(name) {
                self.recursive_templates.insert(name.to_string());
                return Ok(self.start_states.get(name).cloned().unwrap_or(start_state));
            }
        }

        let raw_nodes = self
            .raw_templates
            .get(name)
            .cloned()
            .ok_or_else(|| TemplateError::Parse(format!("no such template `{name}`")))?;

        self.start_states
            .insert(name.to_string(), start_state.clone());
        self.in_progress.insert(name.to_string());

        let analysis = (|| -> Result<(Vec<Node>, ContextState)> {
            let mut nodes = raw_nodes;
            let start_tracker = ContextTracker::from_state(start_state.clone());
            let flows = self.analyze_nodes(&mut nodes, start_tracker, false)?;

            let mut normal_states = HashSet::new();
            for flow in &flows {
                if flow.kind == AnalysisFlowKind::Normal {
                    normal_states.insert(flow.tracker.state());
                }
            }

            if normal_states.len() != 1 {
                if self.recursive_templates.contains(name) {
                    normal_states.clear();
                    normal_states.insert(start_state.clone());
                } else {
                    return Err(TemplateError::Parse(format!(
                        "cannot compute output context for template `{name}`"
                    )));
                }
            }

            for flow in &flows {
                if flow.kind != AnalysisFlowKind::Normal {
                    return Err(TemplateError::Parse(
                        "break/continue action is not inside range".to_string(),
                    ));
                }
            }

            let end_state = normal_states
                .into_iter()
                .next()
                .unwrap_or_else(ContextState::html_text);
            Ok((nodes, end_state))
        })();

        self.in_progress.remove(name);

        let (nodes, end_state) = analysis?;
        self.analyzed_templates.insert(name.to_string(), nodes);
        self.end_states.insert(name.to_string(), end_state.clone());
        Ok(end_state)
    }

    fn analyze_nodes(
        &mut self,
        nodes: &mut [Node],
        start_tracker: ContextTracker,
        in_range: bool,
    ) -> Result<Vec<AnalysisFlow>> {
        let mut flows = vec![AnalysisFlow::normal(start_tracker)];

        for node in nodes {
            let mut next_flows = Vec::new();
            for flow in flows {
                if flow.kind != AnalysisFlowKind::Normal {
                    next_flows.push(flow);
                    continue;
                }

                let mut produced = self.analyze_node(node, flow.tracker, in_range)?;
                next_flows.append(&mut produced);
            }
            flows = dedup_analysis_flows(next_flows);
        }

        Ok(flows)
    }

    fn analyze_node(
        &mut self,
        node: &mut Node,
        mut tracker: ContextTracker,
        in_range: bool,
    ) -> Result<Vec<AnalysisFlow>> {
        match node {
            Node::Text(text) => {
                tracker.append_text(text);
                Ok(vec![AnalysisFlow::normal(tracker)])
            }
            Node::Expr { expr: _, mode, .. } => {
                validate_action_context_before_insertion(&tracker)?;
                let escape_mode = tracker.mode();
                *mode = escape_mode;
                tracker.append_expr_placeholder(escape_mode);
                Ok(vec![AnalysisFlow::normal(tracker)])
            }
            Node::SetVar { .. } | Node::Define { .. } => Ok(vec![AnalysisFlow::normal(tracker)]),
            Node::If {
                then_branch,
                else_branch,
                ..
            } => {
                let then_flows = self.analyze_nodes(then_branch, tracker.clone(), in_range)?;
                let else_flows = self.analyze_nodes(else_branch, tracker, in_range)?;
                ensure_branch_normal_context("if", &then_flows, &else_flows)?;
                let mut merged = then_flows;
                merged.extend(else_flows);
                Ok(dedup_analysis_flows(merged))
            }
            Node::With {
                body, else_branch, ..
            } => {
                let body_flows = self.analyze_nodes(body, tracker.clone(), in_range)?;
                let else_flows = self.analyze_nodes(else_branch, tracker, in_range)?;
                ensure_branch_normal_context("with", &body_flows, &else_flows)?;
                let mut merged = body_flows;
                merged.extend(else_flows);
                Ok(dedup_analysis_flows(merged))
            }
            Node::Range {
                body, else_branch, ..
            } => {
                let range_start = tracker.clone();
                let body_flows = self.analyze_nodes(body, range_start.clone(), true)?;
                let else_flows = self.analyze_nodes(else_branch, range_start.clone(), true)?;

                let mut output_flows = Vec::new();
                let mut natural_exit = true;

                for flow in body_flows {
                    match flow.kind {
                        AnalysisFlowKind::Normal | AnalysisFlowKind::Continue => {}
                        AnalysisFlowKind::Break => {
                            output_flows.push(AnalysisFlow::normal(flow.tracker));
                            natural_exit = false;
                        }
                    }
                }

                if natural_exit || output_flows.is_empty() {
                    output_flows.push(AnalysisFlow::normal(range_start.clone()));
                }

                for flow in else_flows {
                    match flow.kind {
                        AnalysisFlowKind::Normal => output_flows.push(flow),
                        AnalysisFlowKind::Break => {
                            output_flows.push(AnalysisFlow::normal(flow.tracker))
                        }
                        AnalysisFlowKind::Continue => {
                            output_flows.push(AnalysisFlow::normal(range_start.clone()));
                        }
                    }
                }

                ensure_single_normal_context("range", &output_flows)?;
                Ok(dedup_analysis_flows(output_flows))
            }
            Node::TemplateCall { name, .. } => {
                validate_action_context_before_insertion(&tracker)?;
                let start_state = tracker.state();
                let end_state = self.analyze_template(name, start_state.clone())?;
                if start_state == end_state {
                    tracker.append_expr_placeholder(end_state.mode);
                    Ok(vec![AnalysisFlow::normal(tracker)])
                } else {
                    Ok(vec![AnalysisFlow::normal(ContextTracker::from_state(
                        end_state,
                    ))])
                }
            }
            Node::Block { name, body, .. } => {
                if self.raw_templates.contains_key(name) {
                    validate_action_context_before_insertion(&tracker)?;
                    let start_state = tracker.state();
                    let end_state = self.analyze_template(name, start_state)?;
                    Ok(vec![AnalysisFlow::normal(ContextTracker::from_state(
                        end_state,
                    ))])
                } else {
                    self.analyze_nodes(body, tracker, in_range)
                }
            }
            Node::Break => {
                if !in_range {
                    return Err(TemplateError::Parse(
                        "break action is not inside range".to_string(),
                    ));
                }
                Ok(vec![AnalysisFlow::with_kind(
                    AnalysisFlowKind::Break,
                    tracker,
                )])
            }
            Node::Continue => {
                if !in_range {
                    return Err(TemplateError::Parse(
                        "continue action is not inside range".to_string(),
                    ));
                }
                Ok(vec![AnalysisFlow::with_kind(
                    AnalysisFlowKind::Continue,
                    tracker,
                )])
            }
        }
    }
}

fn dedup_analysis_flows(flows: Vec<AnalysisFlow>) -> Vec<AnalysisFlow> {
    let mut deduped = Vec::new();
    let mut seen = HashSet::new();

    for flow in flows {
        let state = flow.tracker.state();
        let rendered_key = match state.mode {
            EscapeMode::ScriptExpr
            | EscapeMode::ScriptTemplate
            | EscapeMode::ScriptRegexp
            | EscapeMode::ScriptLineComment
            | EscapeMode::ScriptBlockComment
            | EscapeMode::StyleExpr
            | EscapeMode::StyleString { .. }
            | EscapeMode::StyleLineComment
            | EscapeMode::StyleBlockComment => flow.tracker.rendered.clone(),
            _ => String::new(),
        };
        let key = (flow.kind, state.clone(), rendered_key.clone());

        if seen.insert(key) {
            if rendered_key.is_empty() {
                deduped.push(AnalysisFlow::with_kind(
                    flow.kind,
                    ContextTracker::from_state(state),
                ));
            } else {
                deduped.push(flow);
            }
        }
    }

    deduped
}

fn ensure_branch_normal_context(
    branch_name: &str,
    left: &[AnalysisFlow],
    right: &[AnalysisFlow],
) -> Result<()> {
    let mut normal_states = HashSet::new();
    for flow in left.iter().chain(right.iter()) {
        if flow.kind == AnalysisFlowKind::Normal {
            normal_states.insert(flow.tracker.state());
        }
    }

    if normal_states.len() > 1 {
        return Err(TemplateError::Parse(format!(
            "{{{{{branch_name}}}}} branches end in different contexts"
        )));
    }

    if has_slash_ambiguity(left.iter().chain(right.iter())) {
        return Err(TemplateError::Parse(
            "'/' could start a division or regexp".to_string(),
        ));
    }

    Ok(())
}

fn ensure_single_normal_context(block_name: &str, flows: &[AnalysisFlow]) -> Result<()> {
    let mut normal_states = HashSet::new();
    for flow in flows {
        if flow.kind == AnalysisFlowKind::Normal {
            normal_states.insert(flow.tracker.state());
        }
    }

    if normal_states.len() > 1 {
        return Err(TemplateError::Parse(format!(
            "{{{{{block_name}}}}} branches end in different contexts"
        )));
    }

    if has_slash_ambiguity(flows.iter()) {
        return Err(TemplateError::Parse(
            "'/' could start a division or regexp".to_string(),
        ));
    }

    Ok(())
}

fn has_slash_ambiguity<'a, I>(flows: I) -> bool
where
    I: IntoIterator<Item = &'a AnalysisFlow>,
{
    let mut js_contexts = HashSet::new();
    for flow in flows {
        if flow.kind != AnalysisFlowKind::Normal {
            continue;
        }
        if !matches!(flow.tracker.state().mode, EscapeMode::ScriptExpr) {
            continue;
        }
        if let Some(context) = script_expr_context(&flow.tracker.rendered) {
            js_contexts.insert(context);
        }
    }
    js_contexts.len() > 1
}

fn is_empty_template_body(nodes: &[Node]) -> bool {
    nodes.iter().all(|node| match node {
        Node::Text(text) => text.trim().is_empty(),
        _ => false,
    })
}

fn validate_function_calls_in_nodes(nodes: &[Node], funcs: &FuncMap) -> Result<()> {
    for node in nodes {
        match node {
            Node::Text(_) | Node::Break | Node::Continue => {}
            Node::Expr { expr, .. } => validate_function_calls_in_expr(expr, funcs)?,
            Node::SetVar { value, .. } => validate_function_calls_in_expr(value, funcs)?,
            Node::If {
                condition,
                then_branch,
                else_branch,
            } => {
                validate_function_calls_in_expr(condition, funcs)?;
                validate_function_calls_in_nodes(then_branch, funcs)?;
                validate_function_calls_in_nodes(else_branch, funcs)?;
            }
            Node::Range {
                iterable,
                body,
                else_branch,
                ..
            } => {
                validate_function_calls_in_expr(iterable, funcs)?;
                validate_function_calls_in_nodes(body, funcs)?;
                validate_function_calls_in_nodes(else_branch, funcs)?;
            }
            Node::With {
                value,
                body,
                else_branch,
            } => {
                validate_function_calls_in_expr(value, funcs)?;
                validate_function_calls_in_nodes(body, funcs)?;
                validate_function_calls_in_nodes(else_branch, funcs)?;
            }
            Node::TemplateCall { data, .. } => {
                if let Some(data) = data {
                    validate_function_calls_in_expr(data, funcs)?;
                }
            }
            Node::Block { data, body, .. } => {
                if let Some(data) = data {
                    validate_function_calls_in_expr(data, funcs)?;
                }
                validate_function_calls_in_nodes(body, funcs)?;
            }
            Node::Define { body, .. } => validate_function_calls_in_nodes(body, funcs)?,
        }
    }
    Ok(())
}

fn validate_function_calls_in_expr(expr: &Expr, funcs: &FuncMap) -> Result<()> {
    for (index, command) in expr.commands.iter().enumerate() {
        match command {
            Command::Value(term) => validate_function_calls_in_term(term, funcs)?,
            Command::Call { name, args } => {
                if name != "call" {
                    if !(index == 0 && args.is_empty()) && !funcs.contains_key(name) {
                        return Err(TemplateError::Parse(format!(
                            "function `{name}` is not registered"
                        )));
                    }
                    // At pipeline head, a bare identifier may resolve from dot/root data
                    // at execution time (Go-compatible missingkey behavior).
                }
                for arg in args {
                    validate_function_calls_in_term(arg, funcs)?;
                }
            }
            Command::Invoke { callee, args } => {
                validate_function_calls_in_term(callee, funcs)?;
                for arg in args {
                    validate_function_calls_in_term(arg, funcs)?;
                }
            }
        }
    }
    Ok(())
}

fn validate_function_calls_in_term(term: &Term, funcs: &FuncMap) -> Result<()> {
    match term {
        Term::SubExpr(expr) => validate_function_calls_in_expr(expr, funcs)?,
        Term::SubExprPath { expr, .. } => validate_function_calls_in_expr(expr, funcs)?,
        _ => {}
    }
    Ok(())
}

fn validate_action_context_before_insertion(tracker: &ContextTracker) -> Result<()> {
    if let Some(prefix) = javascript_prefix_for_context(&tracker.rendered) {
        if has_unfinished_escape(&prefix) {
            return Err(TemplateError::Parse(
                "unfinished escape sequence in JS string".to_string(),
            ));
        }
        if matches!(tracker.mode(), EscapeMode::ScriptRegexp)
            && matches!(
                current_js_scan_state(&prefix),
                JsScanState::RegExp {
                    in_char_class: true,
                    ..
                }
            )
        {
            return Err(TemplateError::Parse(format!(
                "unfinished JS regexp charset: {prefix:?}"
            )));
        }
    }

    if let Some(prefix) = css_prefix_for_context(&tracker.rendered) {
        if has_unfinished_escape(&prefix) {
            return Err(TemplateError::Parse(
                "unfinished escape sequence in CSS string".to_string(),
            ));
        }
    }

    Ok(())
}

fn has_unfinished_escape(prefix: &str) -> bool {
    let mut slash_count = 0usize;
    for ch in prefix.chars().rev() {
        if ch == '\\' {
            slash_count += 1;
        } else {
            break;
        }
    }
    slash_count % 2 == 1
}

fn javascript_prefix_for_context(rendered: &str) -> Option<String> {
    if let Some(context) = current_tag_value_context(rendered) {
        if attr_kind(&context.attr_name) == AttrKind::Js {
            return Some(context.value_prefix);
        }
    }

    let script_tag = current_unclosed_script_tag(rendered)?;
    if !is_script_type_javascript(script_tag) {
        return None;
    }

    current_unclosed_tag_content(rendered, "script").map(str::to_string)
}

fn css_prefix_for_context(rendered: &str) -> Option<String> {
    if let Some(context) = current_tag_value_context(rendered) {
        if attr_kind(&context.attr_name) == AttrKind::Css {
            return Some(context.value_prefix);
        }
    }

    current_unclosed_tag_content(rendered, "style").map(str::to_string)
}

fn script_expr_context(rendered: &str) -> Option<JsContext> {
    let prefix = javascript_prefix_for_context(rendered)?;
    match current_js_scan_state(&prefix) {
        JsScanState::Expr { js_ctx }
        | JsScanState::TemplateExpr { js_ctx, .. }
        | JsScanState::TemplateExprSingleQuote { js_ctx, .. }
        | JsScanState::TemplateExprDoubleQuote { js_ctx, .. }
        | JsScanState::TemplateExprRegExp { js_ctx, .. }
        | JsScanState::TemplateExprTemplateLiteral { js_ctx, .. }
        | JsScanState::TemplateExprLineComment { js_ctx, .. }
        | JsScanState::TemplateExprBlockComment { js_ctx, .. } => Some(js_ctx),
        JsScanState::SingleQuote
        | JsScanState::DoubleQuote
        | JsScanState::RegExp { .. }
        | JsScanState::TemplateLiteral
        | JsScanState::LineComment { .. }
        | JsScanState::BlockComment { .. } => None,
    }
}

fn placeholder_for_mode(mode: EscapeMode) -> &'static str {
    match mode {
        EscapeMode::ScriptExpr => "0",
        EscapeMode::Html
        | EscapeMode::AttrQuoted { .. }
        | EscapeMode::AttrUnquoted { .. }
        | EscapeMode::AttrName
        | EscapeMode::ScriptString { .. }
        | EscapeMode::ScriptTemplate
        | EscapeMode::ScriptRegexp
        | EscapeMode::ScriptLineComment
        | EscapeMode::ScriptBlockComment
        | EscapeMode::StyleExpr
        | EscapeMode::StyleString { .. }
        | EscapeMode::StyleLineComment
        | EscapeMode::StyleBlockComment => "x",
    }
}

fn attr_name_for_kind(kind: AttrKind) -> &'static str {
    match kind {
        AttrKind::Normal => "title",
        AttrKind::Url => "href",
        AttrKind::Js => "onclick",
        AttrKind::Css => "style",
        AttrKind::Srcset => "srcset",
    }
}

fn seed_rendered_for_state(state: &ContextState) -> String {
    match state.mode {
        EscapeMode::Html => {
            if state.in_open_tag {
                "<x".to_string()
            } else {
                String::new()
            }
        }
        EscapeMode::AttrName => "<a x".to_string(),
        EscapeMode::AttrQuoted { kind, quote } => {
            format!("<a {}={quote}x", attr_name_for_kind(kind))
        }
        EscapeMode::AttrUnquoted { kind } => format!("<a {}=x", attr_name_for_kind(kind)),
        EscapeMode::ScriptExpr => "<script>".to_string(),
        EscapeMode::ScriptString { quote } => format!("<script>{quote}"),
        EscapeMode::ScriptTemplate => "<script>`".to_string(),
        EscapeMode::ScriptRegexp => "<script>/x".to_string(),
        EscapeMode::ScriptLineComment => "<script>//".to_string(),
        EscapeMode::ScriptBlockComment => "<script>/*".to_string(),
        EscapeMode::StyleExpr => {
            if state.in_css_attribute {
                let quote = state.css_attribute_quote.unwrap_or('"');
                format!("<a style={quote}")
            } else {
                "<style>".to_string()
            }
        }
        EscapeMode::StyleString { quote } => {
            if state.in_css_attribute {
                let attr_quote = state.css_attribute_quote.unwrap_or('"');
                format!("<a style={attr_quote}{quote}")
            } else {
                format!("<style>{quote}")
            }
        }
        EscapeMode::StyleLineComment => "<style>//".to_string(),
        EscapeMode::StyleBlockComment => "<style>/*".to_string(),
    }
}

fn is_in_unclosed_tag_context(rendered: &str) -> bool {
    let Some(last_lt) = rendered.rfind('<') else {
        return false;
    };
    let last_gt = rendered.rfind('>');
    if let Some(last_gt) = last_gt {
        if last_gt > last_lt {
            return false;
        }
    }

    let fragment = &rendered[last_lt + 1..];
    if fragment.is_empty() {
        return true;
    }

    let first = fragment.chars().next().unwrap_or_default();
    if first == '/' || first == '!' || first == '?' {
        return false;
    }

    true
}

fn builtin_funcs() -> FuncMap {
    let mut funcs = HashMap::new();

    funcs.insert(
        "safe_html".to_string(),
        Arc::new(|args: &[Value]| {
            let value = args.first().map(Value::to_plain_string).unwrap_or_default();
            Ok(Value::safe_html(value))
        }) as Function,
    );

    funcs.insert(
        "html".to_string(),
        Arc::new(|args: &[Value]| {
            let value = args.first().map(Value::to_plain_string).unwrap_or_default();
            Ok(Value::safe_html(escape_html(&value)))
        }) as Function,
    );

    funcs.insert(
        "urlquery".to_string(),
        Arc::new(|args: &[Value]| {
            if args.is_empty() {
                return Ok(Value::from(String::new()));
            }
            let mut combined = String::new();
            for arg in args {
                combined.push_str(&arg.to_plain_string());
            }
            Ok(Value::from(percent_encode_url(&combined)))
        }) as Function,
    );

    funcs.insert(
        "print".to_string(),
        Arc::new(|args: &[Value]| {
            let mut out = String::new();
            for arg in args {
                out.push_str(&arg.to_plain_string());
            }
            Ok(Value::from(out))
        }) as Function,
    );

    funcs.insert(
        "printf".to_string(),
        Arc::new(|args: &[Value]| {
            if args.is_empty() {
                return Ok(Value::from(String::new()));
            }
            let format = args[0].to_plain_string();
            let rendered = format_printf(&format, &args[1..]);
            Ok(Value::from(rendered))
        }) as Function,
    );

    funcs.insert(
        "println".to_string(),
        Arc::new(|args: &[Value]| {
            let rendered = args
                .iter()
                .map(Value::to_plain_string)
                .collect::<Vec<_>>()
                .join(" ");
            Ok(Value::from(format!("{rendered}\n")))
        }) as Function,
    );

    funcs.insert(
        "js".to_string(),
        Arc::new(|args: &[Value]| {
            let encoded = if args.len() == 1 {
                value_to_json_string(&args[0])?
            } else {
                let mut combined = String::new();
                for arg in args {
                    combined.push_str(&arg.to_plain_string());
                }
                serde_json::to_string(&combined)?
            };
            Ok(Value::safe_html(sanitize_json_for_script(&encoded)))
        }) as Function,
    );

    funcs.insert(
        "slice".to_string(),
        Arc::new(|args: &[Value]| {
            if args.is_empty() {
                return Err(TemplateError::Render(
                    "slice expects at least one argument".to_string(),
                ));
            }
            slice_value(&args[0], &args[1..])
        }) as Function,
    );

    funcs.insert(
        "len".to_string(),
        Arc::new(|args: &[Value]| {
            if args.len() != 1 {
                return Err(TemplateError::Render(
                    "len expects exactly one argument".to_string(),
                ));
            }
            let value = &args[0];
            let len = match value {
                Value::SafeHtml(v) => v.len(),
                Value::SafeHtmlAttr(v) => v.len(),
                Value::SafeJs(v) => v.len(),
                Value::SafeCss(v) => v.len(),
                Value::SafeUrl(v) => v.len(),
                Value::SafeSrcset(v) => v.len(),
                Value::Json(JsonValue::Array(v)) => v.len(),
                Value::Json(JsonValue::Object(v)) => v.len(),
                Value::Json(JsonValue::String(v)) => v.len(),
                _ => {
                    return Err(TemplateError::Render(
                        "len supports string, array, map, or safe_html".to_string(),
                    ));
                }
            };
            Ok(Value::from(len as u64))
        }) as Function,
    );

    funcs.insert(
        "not".to_string(),
        Arc::new(|args: &[Value]| {
            if args.len() != 1 {
                return Err(TemplateError::Render(
                    "not expects exactly one argument".to_string(),
                ));
            }
            let value = &args[0];
            Ok(Value::from(!value.truthy()))
        }) as Function,
    );

    funcs.insert(
        "eq".to_string(),
        Arc::new(|args: &[Value]| {
            if args.len() < 2 {
                return Err(TemplateError::Render(
                    "eq expects at least two arguments".to_string(),
                ));
            }

            let first = &args[0];
            let matches = args[1..]
                .iter()
                .any(|candidate| values_equal(first, candidate));
            Ok(Value::from(matches))
        }) as Function,
    );

    funcs.insert(
        "ne".to_string(),
        Arc::new(|args: &[Value]| {
            if args.len() != 2 {
                return Err(TemplateError::Render(
                    "ne expects exactly two arguments".to_string(),
                ));
            }
            Ok(Value::from(!values_equal(&args[0], &args[1])))
        }) as Function,
    );

    funcs.insert(
        "lt".to_string(),
        Arc::new(|args: &[Value]| {
            if args.len() != 2 {
                return Err(TemplateError::Render(
                    "lt expects exactly two arguments".to_string(),
                ));
            }
            Ok(Value::from(
                compare_values(&args[0], &args[1])? == Ordering::Less,
            ))
        }) as Function,
    );

    funcs.insert(
        "le".to_string(),
        Arc::new(|args: &[Value]| {
            if args.len() != 2 {
                return Err(TemplateError::Render(
                    "le expects exactly two arguments".to_string(),
                ));
            }
            let ordering = compare_values(&args[0], &args[1])?;
            Ok(Value::from(
                ordering == Ordering::Less || ordering == Ordering::Equal,
            ))
        }) as Function,
    );

    funcs.insert(
        "gt".to_string(),
        Arc::new(|args: &[Value]| {
            if args.len() != 2 {
                return Err(TemplateError::Render(
                    "gt expects exactly two arguments".to_string(),
                ));
            }
            Ok(Value::from(
                compare_values(&args[0], &args[1])? == Ordering::Greater,
            ))
        }) as Function,
    );

    funcs.insert(
        "ge".to_string(),
        Arc::new(|args: &[Value]| {
            if args.len() != 2 {
                return Err(TemplateError::Render(
                    "ge expects exactly two arguments".to_string(),
                ));
            }
            let ordering = compare_values(&args[0], &args[1])?;
            Ok(Value::from(
                ordering == Ordering::Greater || ordering == Ordering::Equal,
            ))
        }) as Function,
    );

    funcs.insert(
        "index".to_string(),
        Arc::new(|args: &[Value]| {
            if args.len() < 2 {
                return Err(TemplateError::Render(
                    "index expects at least two arguments".to_string(),
                ));
            }

            let mut current = args[0].clone();
            for key in &args[1..] {
                current = index_value(&current, key)?;
            }
            Ok(current)
        }) as Function,
    );

    funcs.insert(
        "and".to_string(),
        Arc::new(|args: &[Value]| {
            if args.is_empty() {
                return Ok(Value::from(true));
            }

            for arg in args {
                if !arg.truthy() {
                    return Ok(arg.clone());
                }
            }

            Ok(args.last().cloned().unwrap_or_else(|| Value::from(true)))
        }) as Function,
    );

    funcs.insert(
        "or".to_string(),
        Arc::new(|args: &[Value]| {
            if args.is_empty() {
                return Ok(Value::from(false));
            }

            for arg in args {
                if arg.truthy() {
                    return Ok(arg.clone());
                }
            }

            Ok(args.last().cloned().unwrap_or_else(|| Value::from(false)))
        }) as Function,
    );

    funcs
}

fn values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::SafeHtml(a), Value::SafeHtml(b)) => a == b,
        (Value::SafeHtmlAttr(a), Value::SafeHtmlAttr(b)) => a == b,
        (Value::SafeJs(a), Value::SafeJs(b)) => a == b,
        (Value::SafeCss(a), Value::SafeCss(b)) => a == b,
        (Value::SafeUrl(a), Value::SafeUrl(b)) => a == b,
        (Value::SafeSrcset(a), Value::SafeSrcset(b)) => a == b,
        (Value::SafeHtml(a), Value::Json(JsonValue::String(b))) => a == b,
        (Value::Json(JsonValue::String(a)), Value::SafeHtml(b)) => a == b,
        (Value::Json(a), Value::Json(b)) => a == b,
        _ => left.to_plain_string() == right.to_plain_string(),
    }
}

fn compare_values(left: &Value, right: &Value) -> Result<Ordering> {
    if let (Some(left_number), Some(right_number)) = (numeric_value(left), numeric_value(right)) {
        return left_number.partial_cmp(&right_number).ok_or_else(|| {
            TemplateError::Render("comparison with NaN is not supported".to_string())
        });
    }

    if let (Some(left_string), Some(right_string)) = (string_value(left), string_value(right)) {
        return Ok(left_string.cmp(&right_string));
    }

    if let (Some(left_bool), Some(right_bool)) = (bool_value(left), bool_value(right)) {
        return Ok(left_bool.cmp(&right_bool));
    }

    Err(TemplateError::Render(format!(
        "cannot compare values `{}` and `{}`",
        left.to_plain_string(),
        right.to_plain_string()
    )))
}

fn numeric_value(value: &Value) -> Option<f64> {
    match value {
        Value::Json(JsonValue::Number(number)) => number.as_f64(),
        _ => None,
    }
}

fn string_value(value: &Value) -> Option<String> {
    match value {
        Value::SafeHtml(text) => Some(text.clone()),
        Value::SafeHtmlAttr(text) => Some(text.clone()),
        Value::SafeJs(text) => Some(text.clone()),
        Value::SafeCss(text) => Some(text.clone()),
        Value::SafeUrl(text) => Some(text.clone()),
        Value::SafeSrcset(text) => Some(text.clone()),
        Value::Json(JsonValue::String(text)) => Some(text.clone()),
        _ => None,
    }
}

fn bool_value(value: &Value) -> Option<bool> {
    match value {
        Value::Json(JsonValue::Bool(value)) => Some(*value),
        _ => None,
    }
}

fn index_value(container: &Value, index: &Value) -> Result<Value> {
    match container {
        Value::Json(JsonValue::Array(items)) => {
            let index = index_to_usize(index)?;
            let value = items.get(index).ok_or_else(|| {
                TemplateError::Render(format!("array index out of range: {index}"))
            })?;
            Ok(Value::Json(value.clone()))
        }
        Value::Json(JsonValue::Object(map)) => {
            let key = index.to_plain_string();
            let value = map
                .get(&key)
                .ok_or_else(|| TemplateError::Render(format!("map key `{key}` is not present")))?;
            Ok(Value::Json(value.clone()))
        }
        Value::Json(JsonValue::String(text)) => {
            let index = index_to_usize(index)?;
            let value = text.chars().nth(index).ok_or_else(|| {
                TemplateError::Render(format!("string index out of range: {index}"))
            })?;
            Ok(Value::from(value.to_string()))
        }
        Value::SafeHtml(text) => {
            let index = index_to_usize(index)?;
            let value = text.chars().nth(index).ok_or_else(|| {
                TemplateError::Render(format!("string index out of range: {index}"))
            })?;
            Ok(Value::from(value.to_string()))
        }
        Value::SafeHtmlAttr(text)
        | Value::SafeJs(text)
        | Value::SafeCss(text)
        | Value::SafeUrl(text)
        | Value::SafeSrcset(text) => {
            let index = index_to_usize(index)?;
            let value = text.chars().nth(index).ok_or_else(|| {
                TemplateError::Render(format!("string index out of range: {index}"))
            })?;
            Ok(Value::from(value.to_string()))
        }
        _ => Err(TemplateError::Render(
            "index supports array, map, or string".to_string(),
        )),
    }
}

fn index_to_usize(value: &Value) -> Result<usize> {
    match value {
        Value::Json(JsonValue::Number(number)) => {
            if let Some(index) = number.as_u64() {
                return Ok(index as usize);
            }
            if let Some(index) = number.as_i64() {
                if index < 0 {
                    return Err(TemplateError::Render(
                        "index must be non-negative".to_string(),
                    ));
                }
                return Ok(index as usize);
            }
            Err(TemplateError::Render(
                "index must be an integer".to_string(),
            ))
        }
        Value::Json(JsonValue::String(value)) => value
            .parse::<usize>()
            .map_err(|_| TemplateError::Render(format!("index `{value}` is not a valid integer"))),
        _ => Err(TemplateError::Render(
            "index argument must be an integer".to_string(),
        )),
    }
}

fn value_to_json_string(value: &Value) -> Result<String> {
    match value {
        Value::Json(json) => Ok(serde_json::to_string(json)?),
        Value::SafeHtml(text) => Ok(serde_json::to_string(text)?),
        Value::SafeHtmlAttr(text) => Ok(serde_json::to_string(text)?),
        Value::SafeJs(text) => Ok(serde_json::to_string(text)?),
        Value::SafeCss(text) => Ok(serde_json::to_string(text)?),
        Value::SafeUrl(text) => Ok(serde_json::to_string(text)?),
        Value::SafeSrcset(text) => Ok(serde_json::to_string(text)?),
        Value::FunctionRef(name) => Ok(serde_json::to_string(&format!("<function:{name}>"))?),
        Value::Missing => Ok(serde_json::to_string("<no value>")?),
    }
}

fn format_printf(format: &str, args: &[Value]) -> String {
    let chars: Vec<char> = format.chars().collect();
    let mut rendered = String::new();
    let mut i = 0usize;
    let mut arg_index = 0usize;

    while i < chars.len() {
        if chars[i] != '%' {
            rendered.push(chars[i]);
            i += 1;
            continue;
        }

        i += 1;
        if i >= chars.len() {
            rendered.push('%');
            break;
        }

        if chars[i] == '%' {
            rendered.push('%');
            i += 1;
            continue;
        }

        let verb = chars[i];
        i += 1;
        let argument = args.get(arg_index);
        if argument.is_some() {
            arg_index += 1;
        }

        match (verb, argument) {
            (_, None) => rendered.push_str("%!missing"),
            ('v', Some(value)) | ('s', Some(value)) => rendered.push_str(&value.to_plain_string()),
            ('q', Some(value)) => {
                let quoted = serde_json::to_string(&value.to_plain_string())
                    .unwrap_or_else(|_| "\"\"".to_string());
                rendered.push_str(&quoted);
            }
            ('d', Some(value)) => rendered.push_str(&format_printf_integer(value)),
            ('f', Some(value)) => rendered.push_str(&format_printf_float(value)),
            ('t', Some(value)) => rendered.push_str(&format_printf_bool(value)),
            (_, Some(value)) => {
                rendered.push('%');
                rendered.push(verb);
                rendered.push_str(&value.to_plain_string());
            }
        }
    }

    if arg_index < args.len() {
        for value in &args[arg_index..] {
            rendered.push_str("%!(EXTRA ");
            rendered.push_str(&value.to_plain_string());
            rendered.push(')');
        }
    }

    rendered
}

fn format_printf_integer(value: &Value) -> String {
    match value {
        Value::Json(JsonValue::Number(number)) => {
            if let Some(i) = number.as_i64() {
                i.to_string()
            } else if let Some(u) = number.as_u64() {
                u.to_string()
            } else if let Some(f) = number.as_f64() {
                (f as i64).to_string()
            } else {
                "0".to_string()
            }
        }
        Value::Json(JsonValue::Bool(value)) => {
            if *value {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        Value::Json(JsonValue::String(value)) => value
            .parse::<i64>()
            .map(|v| v.to_string())
            .unwrap_or_else(|_| "0".to_string()),
        Value::SafeHtml(value)
        | Value::SafeHtmlAttr(value)
        | Value::SafeJs(value)
        | Value::SafeCss(value)
        | Value::SafeUrl(value)
        | Value::SafeSrcset(value) => value
            .parse::<i64>()
            .map(|v| v.to_string())
            .unwrap_or_else(|_| "0".to_string()),
        Value::FunctionRef(_) | Value::Missing | Value::Json(_) => "0".to_string(),
    }
}

fn format_printf_float(value: &Value) -> String {
    match value {
        Value::Json(JsonValue::Number(number)) => number
            .as_f64()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "0".to_string()),
        Value::Json(JsonValue::String(value)) => value
            .parse::<f64>()
            .map(|v| v.to_string())
            .unwrap_or_else(|_| "0".to_string()),
        Value::SafeHtml(value)
        | Value::SafeHtmlAttr(value)
        | Value::SafeJs(value)
        | Value::SafeCss(value)
        | Value::SafeUrl(value)
        | Value::SafeSrcset(value) => value
            .parse::<f64>()
            .map(|v| v.to_string())
            .unwrap_or_else(|_| "0".to_string()),
        Value::Json(JsonValue::Bool(value)) => {
            if *value {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        Value::FunctionRef(_) | Value::Missing | Value::Json(_) => "0".to_string(),
    }
}

fn format_printf_bool(value: &Value) -> String {
    if value.truthy() {
        "true".to_string()
    } else {
        "false".to_string()
    }
}

fn slice_value(base: &Value, indexes: &[Value]) -> Result<Value> {
    match base {
        Value::Json(JsonValue::Array(items)) => {
            let bounds = compute_slice_bounds(items.len(), indexes)?;
            Ok(Value::Json(JsonValue::Array(
                items[bounds.low..bounds.high].to_vec(),
            )))
        }
        Value::Json(JsonValue::String(text)) => {
            let chars = text.chars().collect::<Vec<_>>();
            let bounds = compute_slice_bounds(chars.len(), indexes)?;
            let sliced = chars[bounds.low..bounds.high].iter().collect::<String>();
            Ok(Value::from(sliced))
        }
        Value::SafeHtml(text) => {
            let chars = text.chars().collect::<Vec<_>>();
            let bounds = compute_slice_bounds(chars.len(), indexes)?;
            let sliced = chars[bounds.low..bounds.high].iter().collect::<String>();
            Ok(Value::safe_html(sliced))
        }
        Value::SafeHtmlAttr(text)
        | Value::SafeJs(text)
        | Value::SafeCss(text)
        | Value::SafeUrl(text)
        | Value::SafeSrcset(text) => {
            let chars = text.chars().collect::<Vec<_>>();
            let bounds = compute_slice_bounds(chars.len(), indexes)?;
            let sliced = chars[bounds.low..bounds.high].iter().collect::<String>();
            Ok(Value::from(sliced))
        }
        _ => Err(TemplateError::Render(
            "slice supports array, string, or safe_html".to_string(),
        )),
    }
}

#[derive(Clone, Copy, Debug)]
struct SliceBounds {
    low: usize,
    high: usize,
}

fn compute_slice_bounds(length: usize, indexes: &[Value]) -> Result<SliceBounds> {
    if indexes.len() > 3 {
        return Err(TemplateError::Render(
            "slice supports up to three indexes".to_string(),
        ));
    }

    let low = if let Some(value) = indexes.first() {
        index_to_usize(value)?
    } else {
        0
    };

    let high = if let Some(value) = indexes.get(1) {
        index_to_usize(value)?
    } else {
        length
    };

    if low > high || high > length {
        return Err(TemplateError::Render(format!(
            "invalid slice bounds [{low}:{high}] for length {length}"
        )));
    }

    if let Some(max_value) = indexes.get(2) {
        let max = index_to_usize(max_value)?;
        if high > max || max > length {
            return Err(TemplateError::Render(format!(
                "invalid slice max index {max} for length {length}"
            )));
        }
    }

    Ok(SliceBounds { low, high })
}

pub fn lookup_path(base: &Value, path: &[String]) -> Value {
    if path.is_empty() {
        return base.clone();
    }

    let mut current = match base {
        Value::Json(value) => value,
        Value::SafeHtml(_)
        | Value::SafeHtmlAttr(_)
        | Value::SafeJs(_)
        | Value::SafeCss(_)
        | Value::SafeUrl(_)
        | Value::SafeSrcset(_)
        | Value::FunctionRef(_)
        | Value::Missing => {
            return Value::Json(JsonValue::Null);
        }
    };

    for segment in path {
        match current {
            JsonValue::Object(map) => match map.get(segment) {
                Some(value) => current = value,
                None => return Value::Json(JsonValue::Null),
            },
            JsonValue::Array(items) => {
                let Ok(index) = segment.parse::<usize>() else {
                    return Value::Json(JsonValue::Null);
                };
                match items.get(index) {
                    Some(value) => current = value,
                    None => return Value::Json(JsonValue::Null),
                }
            }
            _ => return Value::Json(JsonValue::Null),
        }
    }

    Value::Json(current.clone())
}

fn lookup_path_with_methods(
    base: &Value,
    path: &[String],
    methods: &MethodMap,
    missing_key_mode: MissingKeyMode,
) -> Result<Value> {
    if path.is_empty() {
        return Ok(base.clone());
    }

    let mut current = base.clone();
    for segment in path {
        current = lookup_single_segment(&current, segment, methods, missing_key_mode)?;
    }
    Ok(current)
}

fn lookup_single_segment(
    current: &Value,
    segment: &str,
    methods: &MethodMap,
    missing_key_mode: MissingKeyMode,
) -> Result<Value> {
    let direct = match current {
        Value::Json(JsonValue::Object(map)) => map.get(segment).cloned().map(Value::Json),
        Value::Json(JsonValue::Array(items)) => segment
            .parse::<usize>()
            .ok()
            .and_then(|index| items.get(index))
            .cloned()
            .map(Value::Json),
        _ => None,
    };

    if let Some(value) = direct {
        return Ok(value);
    }

    if let Some(method) = methods.get(segment) {
        return method(current, &[]);
    }

    match current {
        Value::Json(JsonValue::Object(_)) => missing_value_for_key(segment, missing_key_mode),
        _ => Ok(Value::Json(JsonValue::Null)),
    }
}

fn lookup_identifier(
    dot: &Value,
    root: &Value,
    name: &str,
    methods: &MethodMap,
    _missing_key_mode: MissingKeyMode,
) -> Result<Option<Value>> {
    if let Some(value) = lookup_object_key(dot, name).or_else(|| lookup_object_key(root, name)) {
        return Ok(Some(value));
    }

    if let Some(method) = methods.get(name) {
        return Ok(Some(method(dot, &[])?));
    }

    Ok(None)
}

fn missing_value_for_key(key: &str, mode: MissingKeyMode) -> Result<Value> {
    match mode {
        MissingKeyMode::Default => Ok(Value::Missing),
        MissingKeyMode::Zero => Ok(Value::Json(JsonValue::Null)),
        MissingKeyMode::Error => Err(TemplateError::Render(format!(
            "map has no entry for key `{key}`"
        ))),
    }
}

fn split_last_path(path: &[String]) -> (&str, &[String]) {
    let split_index = path.len() - 1;
    (path[split_index].as_str(), &path[..split_index])
}

fn lookup_variable(scopes: &ScopeStack, name: &str) -> Option<Value> {
    for scope in scopes.iter().rev() {
        if let Some(value) = scope.get(name) {
            return Some(value.clone());
        }
    }
    None
}

fn declare_variable(scopes: &mut ScopeStack, name: &str, value: Value) {
    if scopes.is_empty() {
        scopes.push(HashMap::new());
    }
    if let Some(scope) = scopes.last_mut() {
        scope.insert(name.to_string(), value);
    }
}

fn assign_variable(scopes: &mut ScopeStack, name: &str, value: Value) -> Result<()> {
    for scope in scopes.iter_mut().rev() {
        if scope.contains_key(name) {
            scope.insert(name.to_string(), value);
            return Ok(());
        }
    }

    Err(TemplateError::Render(format!(
        "variable `${name}` is not declared"
    )))
}

fn push_scope(scopes: &mut ScopeStack) {
    scopes.push(HashMap::new());
}

fn pop_scope(scopes: &mut ScopeStack) {
    let _ = scopes.pop();
    if scopes.is_empty() {
        scopes.push(HashMap::new());
    }
}

fn lookup_object_key(value: &Value, name: &str) -> Option<Value> {
    match value {
        Value::Json(JsonValue::Object(map)) => map.get(name).cloned().map(Value::Json),
        _ => None,
    }
}

fn validate_template_hazards(_source: &str) -> Result<()> {
    Ok(())
}

fn contains_pattern_in_tag_content(source: &str, tag_name: &str, pattern: &str) -> bool {
    let lower = source.to_ascii_lowercase();
    let open_pattern = format!("<{tag_name}");
    let close_pattern = format!("</{tag_name}");
    let mut cursor = 0usize;

    while let Some(relative_open) = lower[cursor..].find(&open_pattern) {
        let open = cursor + relative_open;
        let after_open = open + open_pattern.len();
        if let Some(ch) = lower[after_open..].chars().next() {
            if !(ch.is_whitespace() || ch == '>' || ch == '/') {
                cursor = after_open;
                continue;
            }
        }

        let Some(open_end_relative) = lower[after_open..].find('>') else {
            return false;
        };
        let content_start = after_open + open_end_relative + 1;

        let content_end = if let Some(relative_close) = lower[content_start..].find(&close_pattern)
        {
            content_start + relative_close
        } else {
            source.len()
        };

        if source[content_start..content_end].contains(pattern) {
            return true;
        }

        cursor = content_end.saturating_add(close_pattern.len());
    }

    false
}

fn strip_html_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0usize;
    let mut in_script = false;
    let mut in_style = false;

    while cursor < source.len() {
        if in_script && bytes.get(cursor..cursor + 2) == Some(b"</") {
            if matches_html_tag(&bytes[cursor + 2..], b"script") {
                in_script = false;
            }
        } else if in_style && bytes.get(cursor..cursor + 2) == Some(b"</") {
            if matches_html_tag(&bytes[cursor + 2..], b"style") {
                in_style = false;
            }
        }

        if !in_script && !in_style && bytes.get(cursor..cursor + 4) == Some(b"<!--") {
            let after_open = cursor + 4;
            if let Some(end_rel) = source[after_open..].find("-->") {
                cursor = after_open + end_rel + 3;
                continue;
            }
            break;
        }

        if !in_script && !in_style && bytes.get(cursor) == Some(&b'<') {
            if let Some(rest) = bytes.get(cursor + 1..) {
                if !rest.is_empty() && rest[0] == b'/' {
                    if matches_html_tag(&rest[1..], b"script") {
                        in_script = false;
                    } else if matches_html_tag(&rest[1..], b"style") {
                        in_style = false;
                    }
                } else {
                    if matches_html_tag(rest, b"script") {
                        in_script = true;
                    } else if matches_html_tag(rest, b"style") {
                        in_style = true;
                    }
                }
            }
        }

        let next_char_len = source[cursor..]
            .chars()
            .next()
            .map(|ch| ch.len_utf8())
            .unwrap_or(1);
        output.push_str(&source[cursor..cursor + next_char_len]);
        cursor += next_char_len;
    }

    output
}

fn matches_html_tag(bytes: &[u8], name: &[u8]) -> bool {
    if bytes.len() < name.len() {
        return false;
    }
    if !bytes[..name.len()].eq_ignore_ascii_case(name) {
        return false;
    }
    is_html_tag_boundary(bytes.get(name.len()).copied())
}

fn is_html_tag_boundary(byte: Option<u8>) -> bool {
    matches!(
        byte,
        None | Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') | Some(b'>') | Some(b'/')
    )
}

#[cfg(not(feature = "web-rust"))]
fn expand_glob_patterns<I, S>(patterns: I) -> Result<Vec<std::path::PathBuf>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut paths = Vec::new();
    for pattern in patterns {
        for entry in glob::glob(pattern.as_ref())? {
            paths.push(entry?);
        }
    }
    paths.sort();
    if paths.is_empty() {
        return Err(TemplateError::Parse(
            "glob pattern matched no files".to_string(),
        ));
    }
    Ok(paths)
}

#[cfg(not(feature = "web-rust"))]
fn glob_patterns_with_fsys<F, I, S>(fs: &F, patterns: I) -> Result<Vec<String>>
where
    F: TemplateFS,
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut paths = Vec::new();
    for pattern in patterns {
        paths.extend(fs.glob(pattern.as_ref())?);
    }

    paths.sort();
    if paths.is_empty() {
        return Err(TemplateError::Parse(
            "glob pattern matched no files".to_string(),
        ));
    }

    Ok(paths)
}

#[cfg(not(feature = "web-rust"))]
fn parse_files_with_fsys<F>(template: &mut Template, fs: &F, paths: Vec<String>) -> Result<()>
where
    F: TemplateFS,
{
    let mut parsed_any = false;
    for path in paths {
        let path = std::path::Path::new(&path);
        let name = template_name_from_path(path)?;
        let source = fs.read_file(path.to_string_lossy().as_ref())?;
        let source = std::str::from_utf8(&source).map_err(|error| {
            TemplateError::Parse(format!(
                "template file `{path:?}` is not valid UTF-8: {error}"
            ))
        })?;
        template.parse_named(&name, source)?;
        if !template
            .name_space
            .templates
            .read()
            .unwrap()
            .contains_key(&template.name)
        {
            template.name = name;
        }
        parsed_any = true;
    }

    if !parsed_any {
        return Err(TemplateError::Parse(
            "parse_fs requires at least one template".to_string(),
        ));
    }

    template.reanalyze_contexts()?;
    Ok(())
}

fn tokenize(source: &str, left_delim: &str, right_delim: &str) -> Result<Vec<Token>> {
    if left_delim.is_empty() || right_delim.is_empty() {
        return Err(TemplateError::Parse(
            "template delimiters must not be empty".to_string(),
        ));
    }

    let mut tokens = Vec::new();
    let mut cursor = 0usize;

    while let Some(start_offset) = source[cursor..].find(left_delim) {
        let start = cursor + start_offset;
        if start > cursor {
            tokens.push(Token::Text(source[cursor..start].to_string()));
        }

        let mut action_start = start + left_delim.len();
        if source[action_start..].starts_with('-') {
            let mut next_chars = source[action_start + 1..].chars();
            let should_treat_as_unary_minus = match next_chars.next() {
                Some(ch) if ch.is_ascii_digit() || ch == '.' => true,
                Some(_) => false,
                None => false,
            };

            if !should_treat_as_unary_minus {
                action_start += 1;
                trim_last_text_whitespace(&mut tokens);
            }
        }

        let end_offset = source[action_start..].find(right_delim).ok_or_else(|| {
            TemplateError::Parse(format!("unclosed action (missing `{right_delim}`)"))
        })?;
        let end = action_start + end_offset;

        let mut action = source[action_start..end].trim().to_string();
        let trim_right = action.ends_with('-');
        if trim_right {
            action.pop();
            action = action.trim_end().to_string();
        }

        tokens.push(Token::Action(action));
        cursor = end + right_delim.len();

        if trim_right {
            while cursor < source.len() {
                let ch = source[cursor..].chars().next().ok_or_else(|| {
                    TemplateError::Parse("invalid UTF-8 boundary while trimming".to_string())
                })?;
                if ch.is_whitespace() {
                    cursor += ch.len_utf8();
                } else {
                    break;
                }
            }
        }
    }

    if cursor < source.len() {
        tokens.push(Token::Text(source[cursor..].to_string()));
    }

    Ok(tokens)
}

fn trim_last_text_whitespace(tokens: &mut [Token]) {
    if let Some(Token::Text(last)) = tokens.last_mut() {
        let trimmed = last.trim_end().to_string();
        *last = trimmed;
    }
}

fn parse_nodes(
    tokens: &[Token],
    index: &mut usize,
    stop_keywords: &[&str],
) -> Result<(Vec<Node>, Option<StopAction>)> {
    let mut nodes = Vec::new();

    while *index < tokens.len() {
        match &tokens[*index] {
            Token::Text(text) => {
                nodes.push(Node::Text(text.clone()));
                *index += 1;
            }
            Token::Action(raw_action) => {
                let action = raw_action.trim();
                if action.is_empty() {
                    *index += 1;
                    continue;
                }
                if action.starts_with("/*") && action.ends_with("*/") {
                    *index += 1;
                    continue;
                }

                let (head, tail) = split_head(action);

                if stop_keywords.iter().any(|keyword| *keyword == head) {
                    *index += 1;
                    return Ok((
                        nodes,
                        Some(StopAction {
                            keyword: head.to_string(),
                            tail: tail.to_string(),
                        }),
                    ));
                }

                match head {
                    "if" => {
                        if tail.is_empty() {
                            return Err(TemplateError::Parse(
                                "if requires a condition".to_string(),
                            ));
                        }
                        *index += 1;
                        let condition = parse_expression(tail)?;
                        let parsed = parse_if_from_condition(tokens, index, condition)?;
                        nodes.push(parsed);
                    }
                    "range" => {
                        if tail.is_empty() {
                            return Err(TemplateError::Parse(
                                "range requires an expression".to_string(),
                            ));
                        }
                        *index += 1;
                        let (vars, iterable, declare_vars) = parse_range_clause(tail)?;
                        let (body, else_branch) =
                            parse_optional_else_block(tokens, index, "range")?;
                        nodes.push(Node::Range {
                            vars,
                            declare_vars,
                            iterable,
                            body,
                            else_branch,
                        });
                    }
                    "with" => {
                        if tail.is_empty() {
                            return Err(TemplateError::Parse(
                                "with requires an expression".to_string(),
                            ));
                        }
                        *index += 1;
                        let value = parse_expression(tail)?;
                        let parsed = parse_with_from_value(tokens, index, value)?;
                        nodes.push(parsed);
                    }
                    "define" => {
                        let name = parse_quoted_name(tail)?;
                        *index += 1;
                        let (body, stop) = parse_nodes(tokens, index, &["end"])?;
                        match stop {
                            Some(stop) if stop.keyword == "end" => {
                                nodes.push(Node::Define { name, body });
                            }
                            _ => {
                                return Err(TemplateError::Parse(
                                    "define block is missing `end`".to_string(),
                                ));
                            }
                        }
                    }
                    "template" => {
                        let (name, data) = parse_template_call(tail)?;
                        nodes.push(Node::TemplateCall { name, data });
                        *index += 1;
                    }
                    "block" => {
                        if tail.is_empty() {
                            return Err(TemplateError::Parse(
                                "block requires a template name".to_string(),
                            ));
                        }
                        let (name, data) = parse_template_call(tail)?;
                        *index += 1;
                        let (body, stop) = parse_nodes(tokens, index, &["end"])?;
                        match stop {
                            Some(stop) if stop.keyword == "end" => {
                                nodes.push(Node::Block { name, data, body });
                            }
                            _ => {
                                return Err(TemplateError::Parse(
                                    "block is missing `end`".to_string(),
                                ));
                            }
                        }
                    }
                    "break" => {
                        if !tail.is_empty() {
                            return Err(TemplateError::Parse(
                                "break does not accept arguments".to_string(),
                            ));
                        }
                        nodes.push(Node::Break);
                        *index += 1;
                    }
                    "continue" => {
                        if !tail.is_empty() {
                            return Err(TemplateError::Parse(
                                "continue does not accept arguments".to_string(),
                            ));
                        }
                        nodes.push(Node::Continue);
                        *index += 1;
                    }
                    "else" | "end" => {
                        return Err(TemplateError::Parse(format!("unexpected `{head}`")));
                    }
                    _ => {
                        if let Some(set_var) = parse_variable_assignment_action(action)? {
                            nodes.push(set_var);
                        } else {
                            let expr = parse_expression(action)?;
                            nodes.push(Node::Expr {
                                expr,
                                mode: EscapeMode::Html,
                            });
                        }
                        *index += 1;
                    }
                }
            }
        }
    }

    Ok((nodes, None))
}

fn parse_if_from_condition(tokens: &[Token], index: &mut usize, condition: Expr) -> Result<Node> {
    let (then_branch, stop) = parse_nodes(tokens, index, &["else", "end"])?;
    let mut else_branch = Vec::new();

    match stop {
        Some(stop) if stop.keyword == "end" => {}
        Some(stop) if stop.keyword == "else" => {
            if stop.tail.is_empty() {
                let (parsed_else, end) = parse_nodes(tokens, index, &["end"])?;
                match end {
                    Some(end) if end.keyword == "end" => {
                        else_branch = parsed_else;
                    }
                    _ => {
                        return Err(TemplateError::Parse(
                            "if block is missing closing `end`".to_string(),
                        ));
                    }
                }
            } else {
                let (head, tail) = split_head(&stop.tail);
                if head == "if" {
                    if tail.is_empty() {
                        return Err(TemplateError::Parse(
                            "else if requires a condition".to_string(),
                        ));
                    }
                    let else_if_condition = parse_expression(tail)?;
                    let nested = parse_if_from_condition(tokens, index, else_if_condition)?;
                    else_branch.push(nested);
                } else {
                    return Err(TemplateError::Parse(format!(
                        "unsupported else clause `{}`",
                        stop.tail
                    )));
                }
            }
        }
        Some(stop) => {
            return Err(TemplateError::Parse(format!(
                "unexpected control action `{}` in if block",
                stop.keyword
            )));
        }
        None => {
            return Err(TemplateError::Parse(
                "if block is missing `end`".to_string(),
            ));
        }
    }

    Ok(Node::If {
        condition,
        then_branch,
        else_branch,
    })
}

fn parse_with_from_value(tokens: &[Token], index: &mut usize, value: Expr) -> Result<Node> {
    let (body, stop) = parse_nodes(tokens, index, &["else", "end"])?;
    let mut else_branch = Vec::new();

    match stop {
        Some(stop) if stop.keyword == "end" => {}
        Some(stop) if stop.keyword == "else" => {
            if stop.tail.is_empty() {
                let (parsed_else, end) = parse_nodes(tokens, index, &["end"])?;
                match end {
                    Some(end) if end.keyword == "end" => {
                        else_branch = parsed_else;
                    }
                    _ => {
                        return Err(TemplateError::Parse(
                            "with block is missing closing `end`".to_string(),
                        ));
                    }
                }
            } else {
                let (head, tail) = split_head(&stop.tail);
                if head == "with" {
                    if tail.is_empty() {
                        return Err(TemplateError::Parse(
                            "else with requires an expression".to_string(),
                        ));
                    }
                    let else_with_value = parse_expression(tail)?;
                    let nested = parse_with_from_value(tokens, index, else_with_value)?;
                    else_branch.push(nested);
                } else {
                    return Err(TemplateError::Parse(format!(
                        "unsupported else clause `{}`",
                        stop.tail
                    )));
                }
            }
        }
        Some(stop) => {
            return Err(TemplateError::Parse(format!(
                "unexpected control action `{}` in with block",
                stop.keyword
            )));
        }
        None => {
            return Err(TemplateError::Parse(
                "with block is missing `end`".to_string(),
            ));
        }
    }

    Ok(Node::With {
        value,
        body,
        else_branch,
    })
}

fn parse_optional_else_block(
    tokens: &[Token],
    index: &mut usize,
    block_name: &str,
) -> Result<(Vec<Node>, Vec<Node>)> {
    let (body, stop) = parse_nodes(tokens, index, &["else", "end"])?;
    match stop {
        Some(stop) if stop.keyword == "end" => Ok((body, Vec::new())),
        Some(stop) if stop.keyword == "else" => {
            if !stop.tail.is_empty() {
                return Err(TemplateError::Parse(format!(
                    "{block_name} does not support `else {}`",
                    stop.tail
                )));
            }
            let (else_branch, end) = parse_nodes(tokens, index, &["end"])?;
            match end {
                Some(end) if end.keyword == "end" => Ok((body, else_branch)),
                _ => Err(TemplateError::Parse(format!(
                    "{block_name} block is missing `end`"
                ))),
            }
        }
        Some(stop) => Err(TemplateError::Parse(format!(
            "unexpected control action `{}` in {block_name} block",
            stop.keyword
        ))),
        None => Err(TemplateError::Parse(format!(
            "{block_name} block is missing `end`"
        ))),
    }
}

fn split_head(input: &str) -> (&str, &str) {
    let trimmed = input.trim();
    for (index, ch) in trimmed.char_indices() {
        if ch.is_whitespace() {
            return (&trimmed[..index], trimmed[index..].trim());
        }
    }
    (trimmed, "")
}

fn parse_template_call(input: &str) -> Result<(String, Option<Expr>)> {
    let trimmed = input.trim_start();
    let (name, consumed) = parse_string_literal_prefix(trimmed)?;
    let tail = trimmed[consumed..].trim();
    let data = if tail.is_empty() {
        None
    } else {
        Some(parse_expression(tail)?)
    };
    Ok((name, data))
}

fn parse_range_clause(input: &str) -> Result<(Vec<String>, Expr, bool)> {
    if let Some(index) = find_unquoted_operator(input, ":=") {
        let variables = input[..index].trim();
        let expression = input[index + 2..].trim();
        if variables.is_empty() || expression.is_empty() {
            return Err(TemplateError::Parse(
                "range variable declaration must be `<vars> := <expr>`".to_string(),
            ));
        }

        let vars = parse_variable_list(variables, 2)?;
        let iterable = parse_expression(expression)?;
        return Ok((vars, iterable, true));
    }

    if let Some(index) = find_unquoted_operator(input, "=") {
        let variables = input[..index].trim();
        let expression = input[index + 1..].trim();
        if variables.is_empty() || expression.is_empty() {
            return Err(TemplateError::Parse(
                "range variable assignment must be `<vars> = <expr>`".to_string(),
            ));
        }

        let vars = parse_variable_list(variables, 2)?;
        let iterable = parse_expression(expression)?;
        return Ok((vars, iterable, false));
    }

    Ok((Vec::new(), parse_expression(input)?, true))
}

fn parse_variable_assignment_action(action: &str) -> Result<Option<Node>> {
    let declaration_index = find_unquoted_operator(action, ":=");
    let (index, declare) = match declaration_index {
        Some(index) => (index, true),
        None => match find_unquoted_operator(action, "=") {
            Some(index) => (index, false),
            None => return Ok(None),
        },
    };

    let variable = action[..index].trim();
    let expression = if declare {
        action[index + 2..].trim()
    } else {
        action[index + 1..].trim()
    };

    if variable.is_empty() || expression.is_empty() {
        return Err(TemplateError::Parse(
            "variable assignment must be `$name := <expr>` or `$name = <expr>`".to_string(),
        ));
    }

    let name = parse_variable_name(variable)?;
    let value = parse_expression(expression)?;
    Ok(Some(Node::SetVar {
        name,
        value,
        declare,
    }))
}

fn parse_variable_list(input: &str, max_len: usize) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for raw in input.split(',') {
        names.push(parse_variable_name(raw.trim())?);
    }

    if names.is_empty() || names.len() > max_len {
        return Err(TemplateError::Parse(format!(
            "expected between 1 and {max_len} variables"
        )));
    }
    Ok(names)
}

fn parse_variable_name(input: &str) -> Result<String> {
    if !input.starts_with('$') {
        return Err(TemplateError::Parse(format!(
            "variable `{input}` must start with `$`"
        )));
    }
    if input == "$" || input.contains('.') {
        return Err(TemplateError::Parse(format!(
            "invalid variable name `{input}`"
        )));
    }

    let name = &input[1..];
    if !is_identifier(name) {
        return Err(TemplateError::Parse(format!(
            "invalid variable name `{input}`"
        )));
    }
    Ok(name.to_string())
}

fn find_unquoted_operator(input: &str, operator: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (index, ch) in input.char_indices() {
        if let Some(active_quote) = quote {
            if active_quote == '`' {
                if ch == '`' {
                    quote = None;
                }
                continue;
            }

            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == active_quote {
                quote = None;
            }
            continue;
        }

        if ch == '"' || ch == '\'' || ch == '`' {
            quote = Some(ch);
            continue;
        }

        if input[index..].starts_with(operator) {
            return Some(index);
        }
    }

    None
}

fn parse_quoted_name(input: &str) -> Result<String> {
    let trimmed = input.trim();
    let (name, consumed) = parse_string_literal_prefix(trimmed)?;
    if !trimmed[consumed..].trim().is_empty() {
        return Err(TemplateError::Parse(
            "unexpected tokens after quoted template name".to_string(),
        ));
    }
    Ok(name)
}

fn parse_expression(input: &str) -> Result<Expr> {
    let segments = split_pipeline(input)?;
    let mut commands = Vec::new();

    for segment in segments {
        let terms = tokenize_terms(&segment)?;
        if terms.is_empty() {
            return Err(TemplateError::Parse(
                "pipeline segment is empty".to_string(),
            ));
        }

        if is_function_name(&terms[0]) {
            let args = terms[1..]
                .iter()
                .map(|term| parse_term(term))
                .collect::<Result<Vec<_>>>()?;
            commands.push(Command::Call {
                name: terms[0].clone(),
                args,
            });
        } else {
            if terms.len() == 1 {
                commands.push(Command::Value(parse_term(&terms[0])?));
            } else {
                let callee = parse_term(&terms[0])?;
                let args = terms[1..]
                    .iter()
                    .map(|term| parse_term(term))
                    .collect::<Result<Vec<_>>>()?;
                commands.push(Command::Invoke { callee, args });
            }
        }
    }

    validate_predefined_escapers(&commands)?;

    Ok(Expr { commands })
}

fn validate_predefined_escapers(commands: &[Command]) -> Result<()> {
    for (index, command) in commands.iter().enumerate() {
        let Command::Call { name, .. } = command else {
            continue;
        };

        if (name == "html" || name == "urlquery") && index + 1 != commands.len() {
            return Err(TemplateError::Parse(format!(
                "predefined escaper \"{name}\" disallowed in template"
            )));
        }
    }
    Ok(())
}

fn split_pipeline(input: &str) -> Result<Vec<String>> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut paren_depth = 0usize;

    for ch in input.chars() {
        if let Some(active_quote) = quote {
            current.push(ch);
            if active_quote == '`' {
                if ch == '`' {
                    quote = None;
                }
                continue;
            }

            if escaped {
                escaped = false;
                continue;
            }

            if ch == '\\' {
                escaped = true;
                continue;
            }

            if ch == active_quote {
                quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => {
                quote = Some(ch);
                current.push(ch);
            }
            '(' => {
                paren_depth += 1;
                current.push(ch);
            }
            ')' => {
                if paren_depth == 0 {
                    return Err(TemplateError::Parse(
                        "expression has unmatched `)`".to_string(),
                    ));
                }
                paren_depth -= 1;
                current.push(ch);
            }
            '|' if paren_depth == 0 => {
                let segment = current.trim();
                if segment.is_empty() {
                    return Err(TemplateError::Parse(
                        "pipeline contains an empty segment".to_string(),
                    ));
                }
                segments.push(segment.to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if quote.is_some() {
        return Err(TemplateError::Parse(
            "unterminated quoted string".to_string(),
        ));
    }
    if paren_depth != 0 {
        return Err(TemplateError::Parse(
            "expression has unmatched `(`".to_string(),
        ));
    }

    let last = current.trim();
    if last.is_empty() {
        if segments.is_empty() {
            return Err(TemplateError::Parse("expression is empty".to_string()));
        }
        return Err(TemplateError::Parse(
            "pipeline cannot end with `|`".to_string(),
        ));
    }
    segments.push(last.to_string());

    Ok(segments)
}

fn tokenize_terms(input: &str) -> Result<Vec<String>> {
    let mut terms = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut paren_depth = 0usize;

    for ch in input.chars() {
        if let Some(active_quote) = quote {
            current.push(ch);

            if active_quote == '`' {
                if ch == '`' {
                    quote = None;
                }
                continue;
            }

            if escaped {
                escaped = false;
                continue;
            }

            if ch == '\\' {
                escaped = true;
                continue;
            }

            if ch == active_quote {
                quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => {
                quote = Some(ch);
                current.push(ch);
            }
            '(' => {
                paren_depth += 1;
                current.push(ch);
            }
            ')' => {
                if paren_depth == 0 {
                    return Err(TemplateError::Parse(
                        "expression has unmatched `)`".to_string(),
                    ));
                }
                paren_depth -= 1;
                current.push(ch);
            }
            ch if ch.is_whitespace() && paren_depth == 0 => {
                if !current.is_empty() {
                    terms.push(std::mem::take(&mut current));
                }
            }
            ch if ch.is_whitespace() => current.push(ch),
            _ => current.push(ch),
        }
    }

    if quote.is_some() {
        return Err(TemplateError::Parse(
            "unterminated quoted string".to_string(),
        ));
    }
    if paren_depth != 0 {
        return Err(TemplateError::Parse(
            "expression has unmatched `(`".to_string(),
        ));
    }

    if !current.is_empty() {
        terms.push(current);
    }

    Ok(terms)
}

fn parse_term(token: &str) -> Result<Term> {
    if token == "." {
        return Ok(Term::DotPath(Vec::new()));
    }
    if let Some(path) = token.strip_prefix("$.") {
        return Ok(Term::RootPath(parse_path(path)));
    }
    if token == "$" {
        return Ok(Term::RootPath(Vec::new()));
    }
    if token.starts_with('(') {
        let (inner, path) = parse_parenthesized_expression_token(token)?;
        let expr = Box::new(parse_expression(inner)?);
        if path.is_empty() {
            return Ok(Term::SubExpr(expr));
        }
        return Ok(Term::SubExprPath { expr, path });
    }

    if token.starts_with('"') || token.starts_with('\'') || token.starts_with('`') {
        let (text, consumed) = parse_string_literal_prefix(token)?;
        if consumed != token.len() {
            return Err(TemplateError::Parse(format!(
                "invalid string literal `{token}`"
            )));
        }
        return Ok(Term::Literal(Value::from(text)));
    }

    if token == "true" {
        return Ok(Term::Literal(Value::from(true)));
    }
    if token == "false" {
        return Ok(Term::Literal(Value::from(false)));
    }
    if token == "nil" {
        return Ok(Term::Literal(Value::Json(JsonValue::Null)));
    }

    if let Some(value) = parse_number_literal(token) {
        return Ok(Term::Literal(value));
    }

    if token.starts_with('.') {
        if token.chars().nth(1).is_some_and(|ch| ch.is_ascii_digit()) {
            return Err(TemplateError::Parse(format!(
                "unsupported token `{token}` in expression"
            )));
        }
    }

    if let Some(path) = token.strip_prefix('.') {
        return Ok(Term::DotPath(parse_path(path)));
    }
    if let Some(reference) = token.strip_prefix('$') {
        let (name, path) = parse_variable_reference(reference)?;
        return Ok(Term::Variable { name, path });
    }

    if token.len() > 1 && token.as_bytes()[0] == b'0' {
        return Err(TemplateError::Parse(format!(
            "unsupported token `{token}` in expression"
        )));
    }

    if !token.starts_with('+') && !token.starts_with('-') {
        if let Ok(value) = token.parse::<i64>() {
            return Ok(Term::Literal(Value::from(value)));
        }
        if let Ok(value) = token.parse::<u64>() {
            return Ok(Term::Literal(Value::from(value)));
        }
        if let Ok(value) = token.parse::<f64>() {
            return Ok(Term::Literal(Value::from(value)));
        }
    }

    if is_identifier(token) {
        return Ok(Term::Identifier(token.to_string()));
    }

    Err(TemplateError::Parse(format!(
        "unsupported token `{token}` in expression"
    )))
}

fn parse_parenthesized_expression_token(token: &str) -> Result<(&str, Vec<String>)> {
    if !token.starts_with('(') {
        return Err(TemplateError::Parse(format!(
            "unsupported token `{token}` in expression"
        )));
    }

    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut depth = 0usize;
    let mut close_index: Option<usize> = None;

    for (index, ch) in token.char_indices() {
        if let Some(active_quote) = quote {
            if active_quote == '`' {
                if ch == '`' {
                    quote = None;
                }
                continue;
            }

            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == active_quote {
                quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' | '`' => quote = Some(ch),
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    return Err(TemplateError::Parse(
                        "expression has unmatched `)`".to_string(),
                    ));
                }
                depth -= 1;
                if depth == 0 {
                    close_index = Some(index + ch.len_utf8());
                    break;
                }
            }
            _ => {}
        }
    }

    if quote.is_some() {
        return Err(TemplateError::Parse(
            "unterminated quoted string".to_string(),
        ));
    }
    if depth != 0 {
        return Err(TemplateError::Parse(
            "expression has unmatched `(`".to_string(),
        ));
    }
    let close_index = close_index.ok_or_else(|| {
        TemplateError::Parse(format!("unsupported token `{token}` in expression"))
    })?;
    let suffix = token[close_index..].trim();
    let path = if suffix.is_empty() {
        Vec::new()
    } else if let Some(path) = suffix.strip_prefix('.') {
        let parsed = parse_path(path);
        if parsed.is_empty() {
            return Err(TemplateError::Parse(format!(
                "unsupported token `{token}` in expression"
            )));
        }
        parsed
    } else {
        return Err(TemplateError::Parse(format!(
            "unsupported token `{token}` in expression"
        )));
    };

    let inner = token[1..close_index - 1].trim();
    if inner.is_empty() {
        return Err(TemplateError::Parse(
            "parenthesized expression is empty".to_string(),
        ));
    }
    Ok((inner, path))
}

fn parse_number_literal(token: &str) -> Option<Value> {
    if token.is_empty() {
        return None;
    }

    if token.contains('_') && has_invalid_underscore_placement(token) {
        return None;
    }

    let (sign, rest) = match token.as_bytes()[0] as char {
        '+' => (1.0_f64, &token[1..]),
        '-' => (-1.0_f64, &token[1..]),
        _ => (1.0_f64, token),
    };

    let normalized = token_without_underscore(rest);
    if normalized.is_empty() {
        return None;
    }

    if let Some(value) = parse_prefixed_integer_literal(&normalized) {
        return Some(apply_sign_to_numeric_value(value, sign));
    }

    if let Some(value) = parse_decimal_number(&normalized) {
        return Some(apply_sign_to_numeric_value(value, sign));
    }

    if let Some(value) = parse_hex_number(&normalized) {
        return Some(apply_sign_to_numeric_value(value, sign));
    }

    None
}

fn apply_sign_to_numeric_value(value: Value, sign: f64) -> Value {
    if (sign - 1.0).abs() < f64::EPSILON {
        return value;
    }

    match value {
        Value::Json(JsonValue::Number(number)) => {
            if let Some(integer) = number.as_i64() {
                if integer == i64::MIN {
                    return Value::from(-(integer as f64));
                }
                return Value::from(-integer);
            }
            if let Some(integer) = number.as_u64() {
                if let Ok(signed) = i64::try_from(integer) {
                    return Value::from(-signed);
                }
                return Value::from(-(integer as f64));
            }
            if let Some(float) = number.as_f64() {
                return int_or_float_value(-float);
            }
            Value::Json(JsonValue::Number(number))
        }
        _ => value,
    }
}

fn has_invalid_underscore_placement(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let trimmed = raw
        .strip_prefix('+')
        .or_else(|| raw.strip_prefix('-'))
        .unwrap_or(raw);
    let is_hex_prefixed = trimmed.starts_with("0x") || trimmed.starts_with("0X");

    if bytes.is_empty() {
        return false;
    }

    if bytes[0] == b'_' || *bytes.last().unwrap() == b'_' {
        return true;
    }

    for i in 1..bytes.len() {
        if bytes[i - 1] == b'_' && bytes[i] == b'_' {
            return true;
        }
    }

    for i in 0..bytes.len() {
        if bytes[i] != b'_' {
            continue;
        }

        let prev = bytes.get(i.wrapping_sub(1)).copied();
        let next = bytes.get(i + 1).copied();

        if matches!(prev, Some(b'.' | b'+' | b'-'))
            || (!is_hex_prefixed && matches!(prev, Some(b'e' | b'E')))
        {
            return true;
        }
        if matches!(next, Some(b'.' | b'+' | b'-'))
            || (!is_hex_prefixed && matches!(next, Some(b'e' | b'E')))
        {
            return true;
        }
    }

    false
}

fn parse_prefixed_integer_literal(raw: &str) -> Option<Value> {
    if raw.starts_with("0x") || raw.starts_with("0X") {
        if raw.contains('.') || raw.contains('p') || raw.contains('P') {
            return None;
        }
        return parse_radix_integer_literal(&raw[2..], 16);
    }

    if raw.starts_with("0b") || raw.starts_with("0B") {
        return parse_radix_integer_literal(&raw[2..], 2);
    }

    if raw.starts_with("0o") || raw.starts_with("0O") {
        return parse_radix_integer_literal(&raw[2..], 8);
    }

    if raw.len() > 1 && raw.starts_with('0') {
        if raw.contains('.') || raw.contains('e') || raw.contains('E') {
            return None;
        }
        return parse_radix_integer_literal(raw, 8);
    }

    None
}

fn parse_radix_integer_literal(raw: &str, radix: u32) -> Option<Value> {
    let value = parse_radix_integer(raw, radix)?;
    numeric_value_from_u128(value)
}

fn parse_radix_integer(raw: &str, radix: u32) -> Option<u128> {
    if raw.is_empty() {
        return None;
    }

    let mut value: u128 = 0;
    for ch in raw.chars() {
        let digit = hex_digit_value(ch)? as u32;
        if digit >= radix {
            return None;
        }
        value = value
            .checked_mul(radix as u128)?
            .checked_add(digit as u128)?;
    }
    Some(value)
}

fn numeric_value_from_u128(value: u128) -> Option<Value> {
    if let Ok(integer) = i64::try_from(value) {
        return Some(Value::from(integer));
    }
    if let Ok(integer) = u64::try_from(value) {
        return Some(Value::from(integer));
    }
    Some(Value::from(value as f64))
}

fn token_without_underscore(raw: &str) -> String {
    raw.chars().filter(|ch| *ch != '_').collect()
}

fn parse_decimal_number(raw: &str) -> Option<Value> {
    if raw.is_empty() {
        return None;
    }

    if raw.contains('p') || raw.contains('P') {
        return None;
    }

    if raw.contains('.') || raw.contains('e') || raw.contains('E') {
        return raw.parse::<f64>().ok().map(int_or_float_value);
    }

    if raw.len() > 1 && raw.starts_with('0') {
        let value = parse_radix_integer(raw, 8)?;
        return numeric_value_from_u128(value);
    }

    raw.parse::<i64>()
        .ok()
        .map(Value::from)
        .or_else(|| raw.parse::<u64>().ok().map(Value::from))
        .or_else(|| raw.parse::<f64>().ok().map(Value::from))
}

fn parse_hex_number(raw: &str) -> Option<Value> {
    if !(raw.starts_with("0x") || raw.starts_with("0X")) {
        return None;
    }

    let body = &raw[2..];
    if body.is_empty() {
        return None;
    }

    let (mantissa, exponent) = match body.find(|ch| ch == 'p' || ch == 'P') {
        Some(position) => (&body[..position], &body[position + 1..]),
        None => (body, "0"),
    };

    let mut split = mantissa.splitn(2, '.');
    let integer_part = split.next()?;
    let fraction_part = split.next();
    if split.next().is_some() {
        return None;
    }

    if integer_part.is_empty() && fraction_part.is_none() {
        return None;
    }

    let has_fraction = fraction_part.is_some();
    let has_exponent = raw.contains('p') || raw.contains('P');
    if has_fraction && !has_exponent {
        return None;
    }

    let mut value = parse_hex_integer(integer_part)? as f64;
    if let Some(fraction) = fraction_part {
        let mut divisor = 16.0_f64;
        for ch in fraction.chars() {
            let digit = hex_digit_value(ch)? as f64;
            value += digit / divisor;
            divisor *= 16.0;
        }
    }

    if !has_fraction && !has_exponent {
        let int_value = parse_hex_integer(integer_part)?;
        if let Ok(int) = i64::try_from(int_value) {
            return Some(Value::from(int));
        }
        if let Ok(uint) = u64::try_from(int_value) {
            return Some(Value::from(uint));
        }
    }

    let exp = match exponent.parse::<i32>() {
        Ok(exp) => exp,
        Err(_) => return None,
    };

    value *= 2f64.powi(exp);
    Some(int_or_float_value(value))
}

fn int_or_float_value(value: f64) -> Value {
    if !value.is_finite() {
        return Value::from(value);
    }

    if value.fract() != 0.0 {
        return Value::from(value);
    }

    if value >= i64::MIN as f64 && value <= i64::MAX as f64 {
        return Value::from(value as i64);
    }

    if value >= 0.0 && value <= u64::MAX as f64 {
        return Value::from(value as u64);
    }

    Value::from(value)
}

fn parse_hex_integer(raw: &str) -> Option<u128> {
    if raw.is_empty() {
        return Some(0);
    }

    let mut value: u128 = 0;
    for ch in raw.chars() {
        let digit = hex_digit_value(ch)? as u128;
        value = value.checked_mul(16)? + digit as u128;
    }
    Some(value)
}

fn hex_digit_value(ch: char) -> Option<u8> {
    match ch {
        '0'..='9' => Some((ch as u8) - b'0'),
        'a'..='f' => Some(10 + (ch as u8) - b'a'),
        'A'..='F' => Some(10 + (ch as u8) - b'A'),
        _ => None,
    }
}

fn parse_variable_reference(reference: &str) -> Result<(String, Vec<String>)> {
    if reference.is_empty() {
        return Err(TemplateError::Parse(
            "invalid variable reference `$`".to_string(),
        ));
    }

    let (name, path) = if let Some((name, tail)) = reference.split_once('.') {
        (name, parse_path(tail))
    } else {
        (reference, Vec::new())
    };

    if !is_identifier(name) {
        return Err(TemplateError::Parse(format!(
            "invalid variable reference `${reference}`"
        )));
    }

    Ok((name.to_string(), path))
}

fn parse_path(raw_path: &str) -> Vec<String> {
    raw_path
        .split('.')
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>()
}

fn is_identifier(token: &str) -> bool {
    let mut chars = token.chars();
    match chars.next() {
        Some(ch) if ch.is_alphabetic() || ch == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_alphanumeric() || ch == '_')
}

fn is_function_name(token: &str) -> bool {
    is_identifier(token) && token != "true" && token != "false" && token != "nil"
}

fn assert_valid_callable_name(name: &str, kind: &str) {
    if !is_identifier(name) {
        panic!("{kind} name `{name}` is not a valid identifier");
    }
}

fn parse_string_literal_prefix(input: &str) -> Result<(String, usize)> {
    let mut chars = input.char_indices();
    let Some((_, quote)) = chars.next() else {
        return Err(TemplateError::Parse("expected quoted string".to_string()));
    };

    if quote != '"' && quote != '\'' && quote != '`' {
        return Err(TemplateError::Parse("expected quoted string".to_string()));
    }

    let mut output = String::new();
    let mut escaped = false;

    for (index, ch) in chars {
        if quote == '`' {
            if ch == '`' {
                return Ok((output, index + ch.len_utf8()));
            }
            output.push(ch);
            continue;
        }

        if escaped {
            let resolved = match ch {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '\\' => '\\',
                '"' => '"',
                '\'' => '\'',
                other => other,
            };
            output.push(resolved);
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if ch == quote {
            return Ok((output, index + ch.len_utf8()));
        }

        output.push(ch);
    }

    Err(TemplateError::Parse(
        "unterminated string literal".to_string(),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum AttrKind {
    Normal,
    Url,
    Srcset,
    Js,
    Css,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum EscapeMode {
    Html,
    AttrQuoted { kind: AttrKind, quote: char },
    AttrUnquoted { kind: AttrKind },
    AttrName,
    ScriptExpr,
    ScriptString { quote: char },
    ScriptTemplate,
    ScriptRegexp,
    ScriptLineComment,
    ScriptBlockComment,
    StyleExpr,
    StyleString { quote: char },
    StyleLineComment,
    StyleBlockComment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TagValueContext {
    attr_name: String,
    quoted: bool,
    quote: Option<char>,
    value_prefix: String,
}

fn escape_value_for_mode(value: &Value, mode: EscapeMode) -> Result<String> {
    match (value, mode) {
        (Value::SafeHtml(raw), EscapeMode::Html) => return Ok(raw.clone()),
        (Value::SafeHtml(raw), EscapeMode::AttrName) => return Ok(html_name_filter(&raw)),
        (Value::SafeHtmlAttr(raw), EscapeMode::AttrQuoted { .. })
        | (Value::SafeHtmlAttr(raw), EscapeMode::AttrUnquoted { .. }) => return Ok(raw.clone()),
        (Value::SafeJs(raw), EscapeMode::ScriptExpr)
        | (Value::SafeJs(raw), EscapeMode::ScriptTemplate)
        | (Value::SafeJs(raw), EscapeMode::ScriptRegexp)
        | (Value::SafeJs(raw), EscapeMode::ScriptString { .. })
        | (
            Value::SafeJs(raw),
            EscapeMode::AttrQuoted {
                kind: AttrKind::Js, ..
            },
        )
        | (Value::SafeJs(raw), EscapeMode::AttrUnquoted { kind: AttrKind::Js }) => {
            return Ok(raw.clone());
        }
        (Value::SafeCss(raw), EscapeMode::StyleExpr)
        | (Value::SafeCss(raw), EscapeMode::StyleString { .. })
        | (
            Value::SafeCss(raw),
            EscapeMode::AttrQuoted {
                kind: AttrKind::Css,
                ..
            },
        )
        | (
            Value::SafeCss(raw),
            EscapeMode::AttrUnquoted {
                kind: AttrKind::Css,
            },
        ) => {
            return Ok(raw.clone());
        }
        (
            Value::SafeUrl(raw),
            EscapeMode::AttrQuoted {
                kind: AttrKind::Url,
                ..
            },
        )
        | (
            Value::SafeUrl(raw),
            EscapeMode::AttrUnquoted {
                kind: AttrKind::Url,
            },
        ) => {
            return Ok(raw.clone());
        }
        (
            Value::SafeSrcset(raw),
            EscapeMode::AttrQuoted {
                kind: AttrKind::Url,
                ..
            },
        )
        | (
            Value::SafeSrcset(raw),
            EscapeMode::AttrUnquoted {
                kind: AttrKind::Url,
            },
        ) => {
            return Ok(raw.clone());
        }
        (
            Value::SafeSrcset(raw),
            EscapeMode::AttrQuoted {
                kind: AttrKind::Srcset,
                ..
            },
        )
        | (
            Value::SafeSrcset(raw),
            EscapeMode::AttrUnquoted {
                kind: AttrKind::Srcset,
            },
        ) => {
            return Ok(raw.clone());
        }
        _ => {}
    }

    match mode {
        EscapeMode::Html => Ok(escape_html(&value.to_plain_string())),
        EscapeMode::AttrName => Ok(html_name_filter(&value.to_plain_string())),
        EscapeMode::AttrQuoted { kind, quote } => {
            let text = match kind {
                AttrKind::Js => {
                    let escaped = escape_js_string_fragment(&value.to_plain_string(), quote);
                    format!("{quote}{escaped}{quote}")
                }
                _ => transform_attr_value(&value.to_plain_string(), kind, Some(quote)),
            };
            Ok(escape_html(&text))
        }
        EscapeMode::AttrUnquoted { kind } => {
            let text = transform_attr_value(&value.to_plain_string(), kind, None);
            Ok(escape_attr_unquoted(&text))
        }
        EscapeMode::ScriptExpr => escape_script_value(value),
        EscapeMode::ScriptTemplate => Ok(escape_js_string_fragment(&value.to_plain_string(), '`')),
        EscapeMode::ScriptRegexp => Ok(escape_js_string_fragment(&value.to_plain_string(), '/')),
        EscapeMode::ScriptLineComment | EscapeMode::ScriptBlockComment => Ok(String::new()),
        EscapeMode::ScriptString { quote } => {
            Ok(escape_js_string_fragment(&value.to_plain_string(), quote))
        }
        EscapeMode::StyleExpr => Ok(escape_css_text(&value.to_plain_string())),
        EscapeMode::StyleLineComment | EscapeMode::StyleBlockComment => Ok(String::new()),
        EscapeMode::StyleString { quote } => {
            Ok(escape_css_string_fragment(&value.to_plain_string(), quote))
        }
    }
}

fn infer_escape_mode(rendered: &str) -> EscapeMode {
    if let Some(context) = current_tag_value_context(rendered) {
        let kind = attr_kind(&context.attr_name);
        match kind {
            AttrKind::Js => {
                return match script_attribute_mode(&context.value_prefix) {
                    Some(EscapeMode::ScriptExpr)
                        if context.quoted && !context.value_prefix.trim().is_empty() =>
                    {
                        EscapeMode::AttrQuoted {
                            kind,
                            quote: context.quote.unwrap_or('"'),
                        }
                    }
                    Some(mode) => mode,
                    None => EscapeMode::ScriptExpr,
                };
            }
            AttrKind::Css => {
                return match style_attribute_mode(&context.value_prefix) {
                    Some(mode) => mode,
                    None => EscapeMode::StyleExpr,
                };
            }
            _ => {}
        }

        return if context.quoted {
            EscapeMode::AttrQuoted {
                kind,
                quote: context.quote.unwrap_or('"'),
            }
        } else {
            EscapeMode::AttrUnquoted { kind }
        };
    }

    if current_attr_name_context(rendered) {
        return EscapeMode::AttrName;
    }

    if let Some(mode) = script_escape_mode(rendered) {
        return mode;
    }

    if let Some(mode) = style_escape_mode(rendered) {
        return mode;
    }

    EscapeMode::Html
}

fn in_css_attribute_context(rendered: &str) -> bool {
    match current_tag_value_context(rendered) {
        Some(context) => attr_kind(&context.attr_name) == AttrKind::Css,
        None => false,
    }
}

fn current_tag_value_context(rendered: &str) -> Option<TagValueContext> {
    let last_gt = rendered.rfind('>');
    let last_lt = rendered.rfind('<')?;
    if let Some(last_gt) = last_gt {
        if last_gt > last_lt {
            return None;
        }
    }

    let fragment = &rendered[last_lt + 1..];
    parse_open_tag_value_context(fragment)
}

fn current_attr_name_context(rendered: &str) -> bool {
    let last_gt = rendered.rfind('>');
    let last_lt = match rendered.rfind('<') {
        Some(last_lt) => last_lt,
        None => return false,
    };

    if let Some(last_gt) = last_gt {
        if last_gt > last_lt {
            return false;
        }
    }

    let fragment = &rendered[last_lt + 1..];
    if fragment.is_empty() {
        return false;
    }

    let chars: Vec<char> = fragment.chars().collect();
    let mut i = 0usize;

    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if i >= chars.len() {
        return true;
    }

    if chars[i] == '/' || chars[i] == '!' || chars[i] == '?' {
        return false;
    }

    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            return true;
        }
        if chars[i] == '/' || chars[i] == '>' {
            return false;
        }

        let start = i;
        while i < chars.len() {
            let ch = chars[i];
            if ch.is_whitespace() || ch == '=' || ch == '/' || ch == '>' {
                break;
            }
            i += 1;
        }
        if i <= start {
            return false;
        }

        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() || chars[i] == '/' || chars[i] == '>' {
            return true;
        }

        if chars[i] == '=' {
            i += 1;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }

            if i >= chars.len() {
                return false;
            }

            if chars[i] == '"' || chars[i] == '\'' {
                let quote = chars[i];
                i += 1;
                while i < chars.len() && chars[i] != quote {
                    i += 1;
                }
                if i >= chars.len() {
                    return false;
                }
                i += 1;
                continue;
            }

            while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '>' {
                i += 1;
            }

            if i >= chars.len() {
                return false;
            }

            continue;
        }

        return true;
    }

    false
}

fn parse_open_tag_value_context(fragment: &str) -> Option<TagValueContext> {
    let chars: Vec<char> = fragment.chars().collect();
    if chars.is_empty() {
        return None;
    }

    let mut i = 0usize;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if i >= chars.len() {
        return None;
    }
    if chars[i] == '/' || chars[i] == '!' || chars[i] == '?' {
        return None;
    }

    while i < chars.len() {
        let ch = chars[i];
        if ch.is_whitespace() || ch == '/' || ch == '>' {
            break;
        }
        i += 1;
    }

    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        if chars[i] == '/' || chars[i] == '>' {
            break;
        }

        let attr_start = i;
        while i < chars.len() {
            let ch = chars[i];
            if ch.is_whitespace() || ch == '=' || ch == '/' || ch == '>' {
                break;
            }
            i += 1;
        }
        if i <= attr_start {
            break;
        }
        let attr_name: String = chars[attr_start..i].iter().collect();

        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() || chars[i] != '=' {
            continue;
        }
        i += 1;

        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            return Some(TagValueContext {
                attr_name,
                quoted: false,
                quote: None,
                value_prefix: String::new(),
            });
        }

        let quote = chars[i];
        if quote == '"' || quote == '\'' {
            i += 1;
            let value_start = i;
            while i < chars.len() && chars[i] != quote {
                i += 1;
            }
            if i >= chars.len() {
                let partial: String = chars[value_start..].iter().collect();
                if partial.is_empty() || !partial.ends_with("}}") {
                    return Some(TagValueContext {
                        attr_name,
                        quoted: true,
                        quote: Some(quote),
                        value_prefix: partial,
                    });
                }
                return None;
            }
            i += 1;
        } else {
            let value_start = i;
            while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '>' {
                i += 1;
            }
            if i >= chars.len() {
                let partial: String = chars[value_start..].iter().collect();
                if partial.is_empty() || !partial.ends_with("}}") {
                    return Some(TagValueContext {
                        attr_name,
                        quoted: false,
                        quote: None,
                        value_prefix: partial,
                    });
                }
                return None;
            }
        }
    }

    None
}

fn normalize_attr_name_for_context(attr_name: &str) -> (String, bool) {
    let lower = attr_name.to_ascii_lowercase();
    if lower == "xmlns" || lower.starts_with("xmlns:") {
        return (lower, true);
    }

    if let Some((namespace, local)) = lower.split_once(':') {
        if namespace == "xmlns" {
            return (lower, true);
        }
        // Go html/template strips regular namespaces but keeps data- prefix
        // when both namespace and data- are present (e.g. my:data-href).
        return (local.to_string(), false);
    }

    if let Some(stripped) = lower.strip_prefix("data-") {
        return (stripped.to_string(), false);
    }

    (lower, false)
}

fn attr_kind(attr_name: &str) -> AttrKind {
    let (normalized, xmlns_attr) = normalize_attr_name_for_context(attr_name);
    if xmlns_attr {
        return AttrKind::Url;
    }

    match attr_content_type(&normalized) {
        AttrContentType::Plain => AttrKind::Normal,
        AttrContentType::Url => AttrKind::Url,
        AttrContentType::Js => AttrKind::Js,
        AttrContentType::Css => AttrKind::Css,
        AttrContentType::Srcset => AttrKind::Srcset,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttrContentType {
    Plain,
    Url,
    Js,
    Css,
    Srcset,
}

fn attr_content_type(attr_name: &str) -> AttrContentType {
    match attr_name {
        "accept" | "alt" | "autofocus" | "autocomplete" | "autoplay" | "border" | "checked"
        | "cols" | "colspan" | "class" | "contenteditable" | "contextmenu" | "controls"
        | "coords" | "dir" | "dirname" | "disabled" | "draggable" | "dropzone" | "for"
        | "formtarget" | "headers" | "height" | "high" | "hreflang" | "id" | "ismap" | "kind"
        | "label" | "lang" | "language" | "list" | "loop" | "low" | "max" | "maxlength"
        | "media" | "mediagroup" | "min" | "multiple" | "name" | "open" | "optimum"
        | "placeholder" | "preload" | "pubdate" | "radiogroup" | "readonly" | "required"
        | "reversed" | "rows" | "rowspan" | "spellcheck" | "scope" | "scoped" | "seamless"
        | "selected" | "shape" | "size" | "sizes" | "span" | "srcdoc" | "srclang" | "start"
        | "step" | "tabindex" | "target" | "title" | "value" | "width" | "wrap" => {
            AttrContentType::Plain
        }

        "accept-charset" | "async" | "challenge" | "charset" | "crossorigin" | "defer"
        | "enctype" | "form" | "formenctype" | "formmethod" | "formnovalidate" | "http-equiv"
        | "keytype" | "method" | "novalidate" | "rel" | "sandbox" | "type" => {
            AttrContentType::Plain
        }

        "action" | "archive" | "background" | "cite" | "classid" | "codebase" | "data"
        | "formaction" | "href" | "icon" | "longdesc" | "manifest" | "poster" | "profile"
        | "src" | "usemap" => AttrContentType::Url,

        "srcset" => AttrContentType::Srcset,
        "style" => AttrContentType::Css,
        _ => {
            if attr_name.starts_with("on") {
                return AttrContentType::Js;
            }
            if attr_name.contains("src") || attr_name.contains("uri") || attr_name.contains("url") {
                return AttrContentType::Url;
            }
            AttrContentType::Plain
        }
    }
}

fn html_name_filter(input: &str) -> String {
    if input.is_empty() {
        return "#ZgotmplZ".to_string();
    }

    let name = input.to_ascii_lowercase();
    if !name.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return "#ZgotmplZ".to_string();
    }

    if attr_content_type(&name) != AttrContentType::Plain {
        return "#ZgotmplZ".to_string();
    }

    name
}

fn transform_attr_value(value: &str, kind: AttrKind, quote: Option<char>) -> String {
    match kind {
        AttrKind::Normal => value.to_string(),
        AttrKind::Url => normalize_url_for_attribute(value),
        AttrKind::Srcset => filter_srcset_attribute_value(value),
        AttrKind::Js => {
            let q = quote.unwrap_or('"');
            escape_js_string_fragment(value, q)
        }
        AttrKind::Css => match quote {
            Some(q) => escape_css_string_fragment(value, q),
            None => escape_css_text(value),
        },
    }
}

fn script_attribute_mode(value_prefix: &str) -> Option<EscapeMode> {
    Some(current_js_mode(value_prefix))
}

fn style_attribute_mode(value_prefix: &str) -> Option<EscapeMode> {
    Some(current_css_mode(value_prefix))
}

fn script_escape_mode(rendered: &str) -> Option<EscapeMode> {
    let script_tag = current_unclosed_script_tag(rendered)?;
    if !is_script_type_javascript(script_tag) {
        return None;
    }

    let content = current_unclosed_tag_content(rendered, "script")?;
    Some(current_js_mode(content))
}

fn style_escape_mode(rendered: &str) -> Option<EscapeMode> {
    let content = current_unclosed_tag_content(rendered, "style")?;
    Some(current_css_mode(content))
}

fn current_unclosed_script_tag(rendered: &str) -> Option<&str> {
    let mut cursor = 0usize;

    loop {
        let start = find_open_tag(rendered, cursor, b"script")?;
        let end = html_tag_end(rendered, start)?;

        if let Some(close_start) = find_close_tag(rendered, end, b"script") {
            let close_end = html_tag_end(rendered, close_start).unwrap_or(close_start + 1);
            cursor = close_end;
            continue;
        }

        return Some(&rendered[start..end]);
    }
}

fn is_script_type_javascript(script_tag: &str) -> bool {
    match script_type_attribute(script_tag) {
        Some(type_value) => is_js_type_mime(&type_value),
        None => true,
    }
}

fn script_type_attribute(script_tag: &str) -> Option<String> {
    let bytes = script_tag.as_bytes();
    if bytes.len() < 7 || !script_tag[..7].eq_ignore_ascii_case("<script") {
        return None;
    }

    let mut i = 7usize;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }

    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] == b'/' || bytes[i] == b'>' {
            break;
        }

        let name_start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'='
            && bytes[i] != b'/'
            && bytes[i] != b'>'
        {
            i += 1;
        }
        let name = script_tag[name_start..i].to_ascii_lowercase();

        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] == b'/' || bytes[i] == b'>' {
            continue;
        }
        if bytes[i] != b'=' {
            i += 1;
            continue;
        }

        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }

        let value = if bytes[i] == b'\'' || bytes[i] == b'"' {
            let quote = bytes[i];
            i += 1;
            let value_start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            let value = &script_tag[value_start..i];
            if i < bytes.len() {
                i += 1;
            }
            value
        } else {
            let value_start = i;
            while i < bytes.len()
                && !bytes[i].is_ascii_whitespace()
                && bytes[i] != b'/'
                && bytes[i] != b'>'
            {
                i += 1;
            }
            &script_tag[value_start..i]
        };

        if name == "type" {
            return Some(value.trim().to_string());
        }
    }

    None
}

fn is_js_type_mime(type_value: &str) -> bool {
    let mime = type_value
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    matches!(
        mime.as_str(),
        "application/ecmascript"
            | "application/javascript"
            | "application/json"
            | "application/ld+json"
            | "application/x-ecmascript"
            | "application/x-javascript"
            | "module"
            | "text/ecmascript"
            | "text/javascript"
            | "text/javascript1.0"
            | "text/javascript1.1"
            | "text/javascript1.2"
            | "text/javascript1.3"
            | "text/javascript1.4"
            | "text/javascript1.5"
            | "text/jscript"
            | "text/livescript"
            | "text/x-ecmascript"
            | "text/x-javascript"
    )
}

fn current_unclosed_tag_content<'a>(rendered: &'a str, tag_name: &str) -> Option<&'a str> {
    let lower = rendered.to_ascii_lowercase();
    let open_pattern = format!("<{tag_name}");
    let close_pattern = format!("</{tag_name}");

    let mut cursor = 0usize;
    let mut content_start: Option<usize> = None;

    loop {
        if content_start.is_none() {
            let Some(relative) = lower[cursor..].find(&open_pattern) else {
                return None;
            };
            let open_index = cursor + relative;
            let after_open = open_index + open_pattern.len();
            let next = lower[after_open..].chars().next();
            if let Some(next) = next {
                if !(next.is_whitespace() || next == '>' || next == '/') {
                    cursor = after_open;
                    continue;
                }
            } else {
                return None;
            }

            let Some(close_relative) = lower[open_index..].find('>') else {
                return None;
            };
            let start = open_index + close_relative + 1;
            content_start = Some(start);
            cursor = start;
        } else {
            if tag_name == "script" {
                let start = content_start?;
                if let Some(close_start) = find_close_tag(rendered, start, b"script") {
                    let close_end = html_tag_end(rendered, close_start).unwrap_or(close_start + 1);
                    cursor = close_end;
                    content_start = None;
                    continue;
                }

                return Some(&rendered[start..]);
            }
            if tag_name == "style" {
                let start = content_start?;
                if let Some(close_start) = find_style_close_tag(rendered, start) {
                    let close_end = html_tag_end(rendered, close_start).unwrap_or(close_start + 1);
                    cursor = close_end;
                    content_start = None;
                    continue;
                }
                return Some(&rendered[start..]);
            }

            let Some(relative) = lower[cursor..].find(&close_pattern) else {
                let start = content_start?;
                return Some(&rendered[start..]);
            };
            let close_index = cursor + relative;
            let Some(close_end_relative) = lower[close_index..].find('>') else {
                let start = content_start?;
                return Some(&rendered[start..]);
            };
            cursor = close_index + close_end_relative + 1;
            content_start = None;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum JsContext {
    RegExp,
    DivOp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JsScanState {
    Expr {
        js_ctx: JsContext,
    },
    SingleQuote,
    DoubleQuote,
    RegExp {
        in_char_class: bool,
        js_ctx: JsContext,
    },
    TemplateLiteral,
    TemplateExpr {
        brace_depth: usize,
        js_ctx: JsContext,
    },
    TemplateExprSingleQuote {
        brace_depth: usize,
        js_ctx: JsContext,
    },
    TemplateExprDoubleQuote {
        brace_depth: usize,
        js_ctx: JsContext,
    },
    TemplateExprRegExp {
        in_char_class: bool,
        brace_depth: usize,
        js_ctx: JsContext,
    },
    TemplateExprTemplateLiteral {
        brace_depth: usize,
        js_ctx: JsContext,
    },
    TemplateExprLineComment {
        brace_depth: usize,
        js_ctx: JsContext,
        preserve_body: bool,
        keep_terminator: bool,
    },
    TemplateExprBlockComment {
        brace_depth: usize,
        js_ctx: JsContext,
    },
    LineComment {
        js_ctx: JsContext,
        preserve_body: bool,
        keep_terminator: bool,
    },
    BlockComment {
        js_ctx: JsContext,
    },
}

fn current_js_mode(content: &str) -> EscapeMode {
    match current_js_scan_state(content) {
        JsScanState::Expr { .. } => EscapeMode::ScriptExpr,
        JsScanState::SingleQuote => EscapeMode::ScriptString { quote: '\'' },
        JsScanState::DoubleQuote => EscapeMode::ScriptString { quote: '"' },
        JsScanState::RegExp { .. } => EscapeMode::ScriptRegexp,
        JsScanState::TemplateLiteral => EscapeMode::ScriptTemplate,
        JsScanState::TemplateExpr { .. }
        | JsScanState::TemplateExprSingleQuote { .. }
        | JsScanState::TemplateExprDoubleQuote { .. }
        | JsScanState::TemplateExprRegExp { .. }
        | JsScanState::TemplateExprTemplateLiteral { .. }
        | JsScanState::TemplateExprLineComment { .. }
        | JsScanState::TemplateExprBlockComment { .. } => EscapeMode::ScriptExpr,
        JsScanState::LineComment { .. } => EscapeMode::ScriptLineComment,
        JsScanState::BlockComment { .. } => EscapeMode::ScriptBlockComment,
    }
}

fn is_utf8_line_separator_2028(bytes: &[u8], index: usize) -> bool {
    index + 2 < bytes.len()
        && bytes[index] == 0xE2
        && bytes[index + 1] == 0x80
        && bytes[index + 2] == 0xA8
}

fn is_utf8_line_separator_2029(bytes: &[u8], index: usize) -> bool {
    index + 2 < bytes.len()
        && bytes[index] == 0xE2
        && bytes[index + 1] == 0x80
        && bytes[index + 2] == 0xA9
}

fn is_utf8_line_separator(bytes: &[u8], index: usize) -> bool {
    is_utf8_line_separator_2028(bytes, index) || is_utf8_line_separator_2029(bytes, index)
}

fn current_js_scan_state(content: &str) -> JsScanState {
    let bytes = content.as_bytes();
    let mut state = JsScanState::Expr {
        js_ctx: JsContext::RegExp,
    };
    let mut segment_start = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        state = match state {
            JsScanState::Expr { js_ctx } => {
                let ch = bytes[i];
                match ch {
                    b'\'' => {
                        let _ = next_js_ctx(&content[segment_start..i], js_ctx);
                        segment_start = i + 1;
                        i += 1;
                        JsScanState::SingleQuote
                    }
                    b'"' => {
                        let _ = next_js_ctx(&content[segment_start..i], js_ctx);
                        segment_start = i + 1;
                        i += 1;
                        JsScanState::DoubleQuote
                    }
                    b'`' => {
                        let _ = next_js_ctx(&content[segment_start..i], js_ctx);
                        segment_start = i + 1;
                        i += 1;
                        JsScanState::TemplateLiteral
                    }
                    b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                        let js_ctx = next_js_ctx(&content[segment_start..i], js_ctx);
                        i += 2;
                        segment_start = i;
                        JsScanState::LineComment {
                            js_ctx,
                            preserve_body: true,
                            keep_terminator: true,
                        }
                    }
                    b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                        let js_ctx = next_js_ctx(&content[segment_start..i], js_ctx);
                        i += 2;
                        segment_start = i;
                        JsScanState::BlockComment { js_ctx }
                    }
                    b'<' if i + 4 <= bytes.len() && &bytes[i..i + 4] == b"<!--" => {
                        let js_ctx = next_js_ctx(&content[segment_start..i], js_ctx);
                        i += 4;
                        segment_start = i;
                        JsScanState::LineComment {
                            js_ctx,
                            preserve_body: false,
                            keep_terminator: true,
                        }
                    }
                    b'-' if i + 3 <= bytes.len() && &bytes[i..i + 3] == b"-->" => {
                        let js_ctx = next_js_ctx(&content[segment_start..i], js_ctx);
                        i += 3;
                        segment_start = i;
                        JsScanState::LineComment {
                            js_ctx,
                            preserve_body: false,
                            keep_terminator: true,
                        }
                    }
                    b'#' if i + 1 < bytes.len() && bytes[i + 1] == b'!' => {
                        let js_ctx = next_js_ctx(&content[segment_start..i], js_ctx);
                        i += 2;
                        segment_start = i;
                        JsScanState::LineComment {
                            js_ctx,
                            preserve_body: false,
                            keep_terminator: true,
                        }
                    }
                    _ if is_utf8_line_separator_2028(bytes, i) => {
                        let js_ctx = next_js_ctx(&content[segment_start..i], js_ctx);
                        i += 3;
                        segment_start = i;
                        JsScanState::Expr { js_ctx }
                    }
                    _ if is_utf8_line_separator_2029(bytes, i) => {
                        let js_ctx = next_js_ctx(&content[segment_start..i], js_ctx);
                        i += 3;
                        segment_start = i;
                        JsScanState::Expr { js_ctx }
                    }
                    b'/' => {
                        let js_ctx = next_js_ctx(&content[segment_start..i], js_ctx);
                        if js_ctx == JsContext::RegExp {
                            i += 1;
                            segment_start = i;
                            JsScanState::RegExp {
                                in_char_class: false,
                                js_ctx,
                            }
                        } else {
                            i += 1;
                            segment_start = i;
                            JsScanState::Expr { js_ctx }
                        }
                    }
                    _ => {
                        i += 1;
                        JsScanState::Expr { js_ctx }
                    }
                }
            }
            JsScanState::SingleQuote => {
                if bytes[i] == b'\\' {
                    i += 2;
                    JsScanState::SingleQuote
                } else if bytes[i] == b'\'' {
                    i += 1;
                    JsScanState::Expr {
                        js_ctx: JsContext::DivOp,
                    }
                } else {
                    i += 1;
                    JsScanState::SingleQuote
                }
            }
            JsScanState::DoubleQuote => {
                if bytes[i] == b'\\' {
                    i += 2;
                    JsScanState::DoubleQuote
                } else if bytes[i] == b'"' {
                    i += 1;
                    JsScanState::Expr {
                        js_ctx: JsContext::DivOp,
                    }
                } else {
                    i += 1;
                    JsScanState::DoubleQuote
                }
            }
            JsScanState::RegExp {
                mut in_char_class,
                js_ctx,
            } => {
                if bytes[i] == b'\\' {
                    i += 2;
                    JsScanState::RegExp {
                        in_char_class,
                        js_ctx,
                    }
                } else if bytes[i] == b'[' {
                    in_char_class = true;
                    i += 1;
                    JsScanState::RegExp {
                        in_char_class,
                        js_ctx,
                    }
                } else if bytes[i] == b']' {
                    in_char_class = false;
                    i += 1;
                    JsScanState::RegExp {
                        in_char_class,
                        js_ctx,
                    }
                } else if bytes[i] == b'/' && !in_char_class {
                    if is_script_tag_close_in_regexp(bytes, i) {
                        i += 1;
                        JsScanState::RegExp {
                            in_char_class,
                            js_ctx,
                        }
                    } else {
                        i += 1;
                        JsScanState::Expr { js_ctx }
                    }
                } else {
                    i += 1;
                    JsScanState::RegExp {
                        in_char_class,
                        js_ctx,
                    }
                }
            }
            JsScanState::TemplateLiteral => {
                if bytes[i] == b'\\' {
                    i += 2;
                    JsScanState::TemplateLiteral
                } else if bytes[i] == b'`' {
                    i += 1;
                    JsScanState::Expr {
                        js_ctx: JsContext::DivOp,
                    }
                } else if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                    i += 2;
                    JsScanState::TemplateExpr {
                        brace_depth: 1,
                        js_ctx: JsContext::DivOp,
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateLiteral
                }
            }
            JsScanState::TemplateExpr {
                mut brace_depth,
                js_ctx,
            } => {
                if bytes[i] == b'\\' {
                    i += 2;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'\'' {
                    i += 1;
                    JsScanState::TemplateExprSingleQuote {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'"' {
                    i += 1;
                    JsScanState::TemplateExprDoubleQuote {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'`' {
                    i += 1;
                    JsScanState::TemplateExprTemplateLiteral {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    i += 2;
                    JsScanState::TemplateExprLineComment {
                        brace_depth,
                        js_ctx,
                        preserve_body: false,
                        keep_terminator: true,
                    }
                } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                    i += 2;
                    JsScanState::TemplateExprBlockComment {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'{' {
                    brace_depth += 1;
                    i += 1;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'/' {
                    if js_ctx == JsContext::RegExp {
                        i += 1;
                        JsScanState::TemplateExprRegExp {
                            in_char_class: false,
                            brace_depth,
                            js_ctx,
                        }
                    } else {
                        i += 1;
                        JsScanState::TemplateExpr {
                            brace_depth,
                            js_ctx,
                        }
                    }
                } else if bytes[i] == b'}' {
                    if brace_depth > 0 {
                        brace_depth -= 1;
                    }
                    i += 1;
                    if brace_depth == 0 {
                        JsScanState::TemplateLiteral
                    } else {
                        JsScanState::TemplateExpr {
                            brace_depth,
                            js_ctx,
                        }
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                }
            }
            JsScanState::TemplateExprSingleQuote {
                brace_depth,
                js_ctx,
            } => {
                if bytes[i] == b'\\' {
                    i += 2;
                    JsScanState::TemplateExprSingleQuote {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'\'' {
                    i += 1;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx: JsContext::DivOp,
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateExprSingleQuote {
                        brace_depth,
                        js_ctx,
                    }
                }
            }
            JsScanState::TemplateExprDoubleQuote {
                brace_depth,
                js_ctx,
            } => {
                if bytes[i] == b'\\' {
                    i += 2;
                    JsScanState::TemplateExprDoubleQuote {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'"' {
                    i += 1;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx: JsContext::DivOp,
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateExprDoubleQuote {
                        brace_depth,
                        js_ctx,
                    }
                }
            }
            JsScanState::TemplateExprRegExp {
                mut in_char_class,
                brace_depth,
                js_ctx,
            } => {
                if bytes[i] == b'\\' {
                    i += 2;
                    JsScanState::TemplateExprRegExp {
                        in_char_class,
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'[' {
                    in_char_class = true;
                    i += 1;
                    JsScanState::TemplateExprRegExp {
                        in_char_class,
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b']' {
                    in_char_class = false;
                    i += 1;
                    JsScanState::TemplateExprRegExp {
                        in_char_class,
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'/' && !in_char_class {
                    if is_script_tag_close_in_regexp(bytes, i) {
                        i += 1;
                        JsScanState::TemplateExprRegExp {
                            in_char_class,
                            brace_depth,
                            js_ctx,
                        }
                    } else {
                        i += 1;
                        JsScanState::TemplateExpr {
                            brace_depth,
                            js_ctx,
                        }
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateExprRegExp {
                        in_char_class,
                        brace_depth,
                        js_ctx,
                    }
                }
            }
            JsScanState::TemplateExprTemplateLiteral {
                brace_depth,
                js_ctx,
            } => {
                if bytes[i] == b'\\' {
                    i += 2;
                    JsScanState::TemplateExprTemplateLiteral {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'`' {
                    i += 1;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx: JsContext::DivOp,
                    }
                } else if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                    i += 2;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx: JsContext::DivOp,
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateExprTemplateLiteral {
                        brace_depth,
                        js_ctx,
                    }
                }
            }
            JsScanState::TemplateExprLineComment {
                brace_depth,
                js_ctx,
                preserve_body: _,
                keep_terminator: _,
            } => {
                if bytes[i] == b'\n' || bytes[i] == b'\r' {
                    i += 1;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                } else if is_utf8_line_separator(bytes, i) {
                    i += 3;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateExprLineComment {
                        brace_depth,
                        js_ctx,
                        preserve_body: true,
                        keep_terminator: true,
                    }
                }
            }
            JsScanState::TemplateExprBlockComment {
                brace_depth,
                js_ctx,
            } => {
                if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    i += 2;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateExprBlockComment {
                        brace_depth,
                        js_ctx,
                    }
                }
            }
            JsScanState::LineComment {
                js_ctx,
                preserve_body: _,
                keep_terminator: _,
            } => {
                if bytes[i] == b'\n' || bytes[i] == b'\r' {
                    i += 1;
                    JsScanState::Expr { js_ctx }
                } else if is_utf8_line_separator(bytes, i) {
                    i += 3;
                    JsScanState::Expr { js_ctx }
                } else {
                    i += 1;
                    JsScanState::LineComment {
                        js_ctx,
                        preserve_body: true,
                        keep_terminator: true,
                    }
                }
            }
            JsScanState::BlockComment { js_ctx } => {
                if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    i += 2;
                    JsScanState::Expr { js_ctx }
                } else {
                    i += 1;
                    JsScanState::BlockComment { js_ctx }
                }
            }
        };
    }

    state
}

fn filter_script_text(prefix: &str, text: &str) -> String {
    let bytes = text.as_bytes();
    let mut state = current_js_scan_state(prefix);
    let mut i = 0usize;
    let mut output = String::new();

    while i < bytes.len() {
        state = match state {
            JsScanState::Expr { js_ctx } => {
                let ch = bytes[i];
                match ch {
                    b'\'' => {
                        output.push('\'');
                        i += 1;
                        JsScanState::SingleQuote
                    }
                    b'"' => {
                        output.push('"');
                        i += 1;
                        JsScanState::DoubleQuote
                    }
                    b'`' => {
                        output.push('`');
                        i += 1;
                        JsScanState::TemplateLiteral
                    }
                    b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                        output.push_str("//");
                        i += 2;
                        JsScanState::LineComment {
                            js_ctx,
                            preserve_body: true,
                            keep_terminator: true,
                        }
                    }
                    b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                        output.push_str("/*");
                        i += 2;
                        JsScanState::BlockComment { js_ctx }
                    }
                    b'<' if i + 4 <= bytes.len() && &bytes[i..i + 4] == b"<!--" => {
                        i += 4;
                        JsScanState::LineComment {
                            js_ctx,
                            preserve_body: false,
                            keep_terminator: true,
                        }
                    }
                    b'-' if i + 3 <= bytes.len() && &bytes[i..i + 3] == b"-->" => {
                        i += 3;
                        JsScanState::LineComment {
                            js_ctx,
                            preserve_body: false,
                            keep_terminator: true,
                        }
                    }
                    b'#' if i + 1 < bytes.len() && bytes[i + 1] == b'!' => {
                        i += 2;
                        JsScanState::LineComment {
                            js_ctx,
                            preserve_body: false,
                            keep_terminator: true,
                        }
                    }
                    _ if is_utf8_line_separator_2028(bytes, i) => {
                        output.push('\u{2028}');
                        i += 3;
                        JsScanState::Expr { js_ctx }
                    }
                    _ if is_utf8_line_separator_2029(bytes, i) => {
                        output.push('\u{2029}');
                        i += 3;
                        JsScanState::Expr { js_ctx }
                    }
                    b'/' => {
                        i += 1;
                        output.push('/');
                        let regex_state = if js_ctx == JsContext::RegExp {
                            JsScanState::RegExp {
                                in_char_class: false,
                                js_ctx,
                            }
                        } else {
                            JsScanState::Expr { js_ctx }
                        };
                        regex_state
                    }
                    _ => {
                        output.push(bytes[i] as char);
                        i += 1;
                        JsScanState::Expr { js_ctx }
                    }
                }
            }
            JsScanState::SingleQuote => {
                if bytes[i] == b'\\' {
                    output.push('\\');
                    if i + 1 < bytes.len() {
                        output.push(bytes[i + 1] as char);
                        i += 2;
                    } else {
                        i += 1;
                    }
                    JsScanState::SingleQuote
                } else {
                    output.push(bytes[i] as char);
                    if bytes[i] == b'\'' {
                        i += 1;
                        JsScanState::Expr {
                            js_ctx: JsContext::DivOp,
                        }
                    } else {
                        i += 1;
                        JsScanState::SingleQuote
                    }
                }
            }
            JsScanState::DoubleQuote => {
                if bytes[i] == b'\\' {
                    output.push('\\');
                    if i + 1 < bytes.len() {
                        output.push(bytes[i + 1] as char);
                        i += 2;
                    } else {
                        i += 1;
                    }
                    JsScanState::DoubleQuote
                } else {
                    output.push(bytes[i] as char);
                    if bytes[i] == b'"' {
                        i += 1;
                        JsScanState::Expr {
                            js_ctx: JsContext::DivOp,
                        }
                    } else {
                        i += 1;
                        JsScanState::DoubleQuote
                    }
                }
            }
            JsScanState::RegExp {
                mut in_char_class,
                js_ctx,
            } => {
                output.push(bytes[i] as char);
                if bytes[i] == b'\\' {
                    i += 1;
                    if i < bytes.len() {
                        output.push(bytes[i] as char);
                        i += 1;
                    }
                    JsScanState::RegExp {
                        in_char_class,
                        js_ctx,
                    }
                } else if bytes[i] == b'[' {
                    in_char_class = true;
                    i += 1;
                    JsScanState::RegExp {
                        in_char_class,
                        js_ctx,
                    }
                } else if bytes[i] == b']' {
                    in_char_class = false;
                    i += 1;
                    JsScanState::RegExp {
                        in_char_class,
                        js_ctx,
                    }
                } else if bytes[i] == b'/' && !in_char_class {
                    if is_script_tag_close_in_regexp(bytes, i) {
                        output.push('/');
                        i += 1;
                        JsScanState::RegExp {
                            in_char_class,
                            js_ctx,
                        }
                    } else {
                        output.push('/');
                        i += 1;
                        JsScanState::Expr { js_ctx }
                    }
                } else {
                    i += 1;
                    JsScanState::RegExp {
                        in_char_class,
                        js_ctx,
                    }
                }
            }
            JsScanState::TemplateLiteral => {
                if bytes[i] == b'\\' {
                    output.push('\\');
                    if i + 1 < bytes.len() {
                        output.push(bytes[i + 1] as char);
                        i += 2;
                    } else {
                        i += 1;
                    }
                    JsScanState::TemplateLiteral
                } else {
                    if bytes[i] == b'`' {
                        output.push('`');
                        i += 1;
                        JsScanState::Expr {
                            js_ctx: JsContext::DivOp,
                        }
                    } else if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                        i += 2;
                        output.push_str("${");
                        JsScanState::TemplateExpr {
                            brace_depth: 1,
                            js_ctx: JsContext::DivOp,
                        }
                    } else {
                        output.push(bytes[i] as char);
                        i += 1;
                        JsScanState::TemplateLiteral
                    }
                }
            }
            JsScanState::TemplateExpr {
                mut brace_depth,
                js_ctx,
            } => {
                if bytes[i] == b'\\' {
                    i += 1;
                    if i < bytes.len() {
                        output.push(bytes[i - 1] as char);
                        output.push(bytes[i] as char);
                        i += 1;
                    } else {
                        output.push('\\');
                    }
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'\'' {
                    i += 1;
                    output.push('\'');
                    JsScanState::TemplateExprSingleQuote {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'"' {
                    i += 1;
                    output.push('"');
                    JsScanState::TemplateExprDoubleQuote {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'`' {
                    i += 1;
                    output.push('`');
                    JsScanState::TemplateExprTemplateLiteral {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    i += 2;
                    JsScanState::TemplateExprLineComment {
                        brace_depth,
                        js_ctx,
                        preserve_body: false,
                        keep_terminator: true,
                    }
                } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                    i += 2;
                    JsScanState::TemplateExprBlockComment {
                        brace_depth,
                        js_ctx,
                    }
                } else if is_utf8_line_separator_2028(bytes, i) {
                    output.push('\u{2028}');
                    i += 3;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                } else if is_utf8_line_separator_2029(bytes, i) {
                    output.push('\u{2029}');
                    i += 3;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'{' {
                    brace_depth += 1;
                    i += 1;
                    output.push('{');
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'/' {
                    if js_ctx == JsContext::RegExp {
                        i += 1;
                        output.push('/');
                        JsScanState::TemplateExprRegExp {
                            in_char_class: false,
                            brace_depth,
                            js_ctx,
                        }
                    } else {
                        i += 1;
                        output.push('/');
                        JsScanState::TemplateExpr {
                            brace_depth,
                            js_ctx,
                        }
                    }
                } else if bytes[i] == b'}' {
                    if brace_depth > 0 {
                        brace_depth -= 1;
                    }
                    i += 1;
                    output.push('}');
                    if bytes[i - 1] == b'}' && brace_depth == 0 {
                        JsScanState::TemplateLiteral
                    } else {
                        JsScanState::TemplateExpr {
                            brace_depth,
                            js_ctx,
                        }
                    }
                } else {
                    i += 1;
                    output.push(bytes[i - 1] as char);
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                }
            }
            JsScanState::TemplateExprSingleQuote {
                brace_depth,
                js_ctx,
            } => {
                output.push(bytes[i] as char);
                if bytes[i] == b'\\' {
                    i += 1;
                    if i < bytes.len() {
                        output.push(bytes[i] as char);
                        i += 1;
                    }
                    JsScanState::TemplateExprSingleQuote {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'\'' {
                    i += 1;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx: JsContext::DivOp,
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateExprSingleQuote {
                        brace_depth,
                        js_ctx,
                    }
                }
            }
            JsScanState::TemplateExprDoubleQuote {
                brace_depth,
                js_ctx,
            } => {
                output.push(bytes[i] as char);
                if bytes[i] == b'\\' {
                    i += 1;
                    if i < bytes.len() {
                        output.push(bytes[i] as char);
                        i += 1;
                    }
                    JsScanState::TemplateExprDoubleQuote {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'"' {
                    i += 1;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx: JsContext::DivOp,
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateExprDoubleQuote {
                        brace_depth,
                        js_ctx,
                    }
                }
            }
            JsScanState::TemplateExprRegExp {
                mut in_char_class,
                brace_depth,
                js_ctx,
            } => {
                output.push(bytes[i] as char);
                if bytes[i] == b'\\' {
                    i += 1;
                    if i < bytes.len() {
                        output.push(bytes[i] as char);
                        i += 1;
                    }
                    JsScanState::TemplateExprRegExp {
                        in_char_class,
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'[' {
                    in_char_class = true;
                    i += 1;
                    JsScanState::TemplateExprRegExp {
                        in_char_class,
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b']' {
                    in_char_class = false;
                    i += 1;
                    JsScanState::TemplateExprRegExp {
                        in_char_class,
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'/' && !in_char_class {
                    if is_script_tag_close_in_regexp(bytes, i) {
                        output.push('/');
                        i += 1;
                        JsScanState::TemplateExprRegExp {
                            in_char_class,
                            brace_depth,
                            js_ctx,
                        }
                    } else {
                        output.push('/');
                        i += 1;
                        JsScanState::TemplateExpr {
                            brace_depth,
                            js_ctx,
                        }
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateExprRegExp {
                        in_char_class,
                        brace_depth,
                        js_ctx,
                    }
                }
            }
            JsScanState::TemplateExprTemplateLiteral {
                brace_depth,
                js_ctx,
            } => {
                if bytes[i] == b'\\' {
                    output.push('\\');
                    if i + 1 < bytes.len() {
                        output.push(bytes[i + 1] as char);
                        i += 2;
                    } else {
                        i += 1;
                    }
                    JsScanState::TemplateExprTemplateLiteral {
                        brace_depth,
                        js_ctx,
                    }
                } else {
                    if bytes[i] == b'`' {
                        output.push('`');
                        i += 1;
                        JsScanState::TemplateExpr {
                            brace_depth,
                            js_ctx: JsContext::DivOp,
                        }
                    } else if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                        i += 2;
                        output.push_str("${");
                        JsScanState::TemplateExpr {
                            brace_depth,
                            js_ctx: JsContext::DivOp,
                        }
                    } else {
                        output.push(bytes[i] as char);
                        i += 1;
                        JsScanState::TemplateExprTemplateLiteral {
                            brace_depth,
                            js_ctx,
                        }
                    }
                }
            }
            JsScanState::TemplateExprLineComment {
                brace_depth,
                js_ctx,
                preserve_body,
                keep_terminator,
            } => {
                if bytes[i] == b'\n' || bytes[i] == b'\r' {
                    if keep_terminator {
                        output.push(bytes[i] as char);
                    }
                    i += 1;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == 0xE2
                    && i + 2 < bytes.len()
                    && ((bytes[i + 1] == 0x80 && bytes[i + 2] == 0xA8)
                        || (bytes[i + 1] == 0x80 && bytes[i + 2] == 0xA9))
                {
                    if keep_terminator {
                        if bytes[i + 1] == 0x80 && bytes[i + 2] == 0xA8 {
                            output.push('\u{2028}');
                        } else {
                            output.push('\u{2029}');
                        }
                    }
                    i += 3;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                } else if preserve_body {
                    output.push(bytes[i] as char);
                    i += 1;
                    JsScanState::TemplateExprLineComment {
                        brace_depth,
                        js_ctx,
                        preserve_body: true,
                        keep_terminator: true,
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateExprLineComment {
                        brace_depth,
                        js_ctx,
                        preserve_body: false,
                        keep_terminator: true,
                    }
                }
            }
            JsScanState::TemplateExprBlockComment {
                brace_depth,
                js_ctx,
            } => {
                if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    i += 2;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                } else {
                    if bytes[i].is_ascii_whitespace()
                        && !(bytes[i] == b' '
                            && i + 2 < bytes.len()
                            && bytes[i + 1] == b'*'
                            && bytes[i + 2] == b'/')
                    {
                        output.push(bytes[i] as char);
                    }
                    i += 1;
                    JsScanState::TemplateExprBlockComment {
                        brace_depth,
                        js_ctx,
                    }
                }
            }
            JsScanState::LineComment {
                js_ctx,
                preserve_body,
                keep_terminator,
            } => {
                if bytes[i] == b'\n' || bytes[i] == b'\r' {
                    if keep_terminator {
                        output.push(bytes[i] as char);
                    }
                    i += 1;
                    JsScanState::Expr { js_ctx }
                } else if bytes[i] == 0xE2
                    && i + 2 < bytes.len()
                    && ((bytes[i + 1] == 0x80 && bytes[i + 2] == 0xA8)
                        || (bytes[i + 1] == 0x80 && bytes[i + 2] == 0xA9))
                {
                    let is_2028 = bytes[i + 1] == 0x80 && bytes[i + 2] == 0xA8;
                    if keep_terminator {
                        if is_2028 {
                            output.push('\u{2028}');
                        } else {
                            output.push('\u{2029}');
                        }
                    }
                    i += 3;
                    JsScanState::Expr { js_ctx }
                } else {
                    if preserve_body {
                        output.push(bytes[i] as char);
                    }
                    i += 1;
                    JsScanState::LineComment {
                        js_ctx,
                        preserve_body,
                        keep_terminator,
                    }
                }
            }
            JsScanState::BlockComment { js_ctx } => {
                if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    output.push_str("*/");
                    i += 2;
                    JsScanState::Expr { js_ctx }
                } else {
                    output.push(bytes[i] as char);
                    i += 1;
                    JsScanState::BlockComment { js_ctx }
                }
            }
        };
    }

    output
}

#[derive(Clone, Copy)]
enum HtmlSection {
    Html,
    Script,
    Style,
}

fn html_tag_end(content: &str, start: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    if bytes.get(start) != Some(&b'<') {
        return None;
    }

    let mut i = start + 1;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        match (quote, bytes[i]) {
            (Some(_), b'\\') => {
                if i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                i += 1;
            }
            (Some(q), _) => {
                if bytes[i] == q {
                    quote = None;
                }
                i += 1;
            }
            (None, b'"') | (None, b'\'') => {
                quote = Some(bytes[i]);
                i += 1;
            }
            (None, b'>') => {
                return Some(i + 1);
            }
            (None, _) => {
                i += 1;
            }
        }
    }

    None
}

fn find_open_tag(content: &str, start: usize, tag: &[u8]) -> Option<usize> {
    let bytes = content.as_bytes();
    if start >= bytes.len() {
        return None;
    }

    let mut i = start;
    while i + 1 < bytes.len() {
        if bytes[i] == b'<' && bytes.get(i + 1) != Some(&b'/') {
            if matches_html_tag(&bytes[i + 1..], tag) {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn find_close_tag(content: &str, start: usize, tag: &[u8]) -> Option<usize> {
    if tag == b"script" {
        return find_script_close_tag(content, start);
    }
    if tag == b"style" {
        return find_style_close_tag(content, start);
    }

    let bytes = content.as_bytes();
    if start >= bytes.len() {
        return None;
    }

    let mut i = start;
    while i + 2 < bytes.len() {
        if bytes[i] == b'<' && bytes[i + 1] == b'/' && matches_html_tag(&bytes[i + 2..], tag) {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn find_script_close_tag(content: &str, start: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    if start >= bytes.len() {
        return None;
    }

    let mut state = JsScanState::Expr {
        js_ctx: JsContext::RegExp,
    };
    let mut segment_start = start;
    let mut i = start;

    while i < bytes.len() {
        state = match state {
            JsScanState::Expr { js_ctx } => {
                if is_html_close_tag(bytes, i, b"script") {
                    return Some(i);
                }

                let ch = bytes[i];
                match ch {
                    b'\'' => {
                        let _ = next_js_ctx(&content[segment_start..i], js_ctx);
                        segment_start = i + 1;
                        i += 1;
                        JsScanState::SingleQuote
                    }
                    b'"' => {
                        let _ = next_js_ctx(&content[segment_start..i], js_ctx);
                        segment_start = i + 1;
                        i += 1;
                        JsScanState::DoubleQuote
                    }
                    b'`' => {
                        let _ = next_js_ctx(&content[segment_start..i], js_ctx);
                        segment_start = i + 1;
                        i += 1;
                        JsScanState::TemplateLiteral
                    }
                    b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                        let js_ctx = next_js_ctx(&content[segment_start..i], js_ctx);
                        i += 2;
                        segment_start = i;
                        JsScanState::LineComment {
                            js_ctx,
                            preserve_body: true,
                            keep_terminator: true,
                        }
                    }
                    b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                        let js_ctx = next_js_ctx(&content[segment_start..i], js_ctx);
                        i += 2;
                        segment_start = i;
                        JsScanState::BlockComment { js_ctx }
                    }
                    b'<' if i + 4 <= bytes.len() && &bytes[i..i + 4] == b"<!--" => {
                        let js_ctx = next_js_ctx(&content[segment_start..i], js_ctx);
                        i += 4;
                        segment_start = i;
                        JsScanState::LineComment {
                            js_ctx,
                            preserve_body: false,
                            keep_terminator: true,
                        }
                    }
                    b'-' if i + 3 <= bytes.len() && &bytes[i..i + 3] == b"-->" => {
                        let js_ctx = next_js_ctx(&content[segment_start..i], js_ctx);
                        i += 3;
                        segment_start = i;
                        JsScanState::LineComment {
                            js_ctx,
                            preserve_body: false,
                            keep_terminator: true,
                        }
                    }
                    b'#' if i + 1 < bytes.len() && bytes[i + 1] == b'!' => {
                        let js_ctx = next_js_ctx(&content[segment_start..i], js_ctx);
                        i += 2;
                        segment_start = i;
                        JsScanState::LineComment {
                            js_ctx,
                            preserve_body: false,
                            keep_terminator: true,
                        }
                    }
                    b'/' => {
                        let js_ctx = next_js_ctx(&content[segment_start..i], js_ctx);
                        if js_ctx == JsContext::RegExp {
                            i += 1;
                            segment_start = i;
                            JsScanState::RegExp {
                                in_char_class: false,
                                js_ctx,
                            }
                        } else {
                            i += 1;
                            segment_start = i;
                            JsScanState::Expr { js_ctx }
                        }
                    }
                    _ => {
                        i += 1;
                        JsScanState::Expr { js_ctx }
                    }
                }
            }
            JsScanState::SingleQuote => {
                if bytes[i] == b'\\' {
                    i += 2;
                    JsScanState::SingleQuote
                } else if bytes[i] == b'\'' {
                    i += 1;
                    JsScanState::Expr {
                        js_ctx: JsContext::DivOp,
                    }
                } else {
                    i += 1;
                    JsScanState::SingleQuote
                }
            }
            JsScanState::DoubleQuote => {
                if bytes[i] == b'\\' {
                    i += 2;
                    JsScanState::DoubleQuote
                } else if bytes[i] == b'"' {
                    i += 1;
                    JsScanState::Expr {
                        js_ctx: JsContext::DivOp,
                    }
                } else {
                    i += 1;
                    JsScanState::DoubleQuote
                }
            }
            JsScanState::RegExp {
                mut in_char_class,
                js_ctx,
            } => {
                if bytes[i] == b'\\' {
                    i += 2;
                    JsScanState::RegExp {
                        in_char_class,
                        js_ctx,
                    }
                } else if bytes[i] == b'[' {
                    in_char_class = true;
                    i += 1;
                    JsScanState::RegExp {
                        in_char_class,
                        js_ctx,
                    }
                } else if bytes[i] == b']' {
                    in_char_class = false;
                    i += 1;
                    JsScanState::RegExp {
                        in_char_class,
                        js_ctx,
                    }
                } else if bytes[i] == b'/' && !in_char_class {
                    if is_script_tag_close_in_regexp(bytes, i) {
                        i += 1;
                        JsScanState::RegExp {
                            in_char_class,
                            js_ctx,
                        }
                    } else {
                        i += 1;
                        JsScanState::Expr { js_ctx }
                    }
                } else {
                    i += 1;
                    JsScanState::RegExp {
                        in_char_class,
                        js_ctx,
                    }
                }
            }
            JsScanState::TemplateLiteral => {
                if bytes[i] == b'\\' {
                    i += 2;
                    JsScanState::TemplateLiteral
                } else if bytes[i] == b'`' {
                    i += 1;
                    JsScanState::Expr {
                        js_ctx: JsContext::DivOp,
                    }
                } else if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                    i += 2;
                    JsScanState::TemplateExpr {
                        brace_depth: 1,
                        js_ctx: JsContext::DivOp,
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateLiteral
                }
            }
            JsScanState::TemplateExpr {
                mut brace_depth,
                js_ctx,
            } => {
                if bytes[i] == b'\\' {
                    i += 2;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'\'' {
                    i += 1;
                    JsScanState::TemplateExprSingleQuote {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'"' {
                    i += 1;
                    JsScanState::TemplateExprDoubleQuote {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'`' {
                    i += 1;
                    JsScanState::TemplateExprTemplateLiteral {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    i += 2;
                    JsScanState::TemplateExprLineComment {
                        brace_depth,
                        js_ctx,
                        preserve_body: true,
                        keep_terminator: true,
                    }
                } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                    i += 2;
                    JsScanState::TemplateExprBlockComment {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'{' {
                    brace_depth += 1;
                    i += 1;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'/' {
                    if js_ctx == JsContext::RegExp {
                        i += 1;
                        JsScanState::TemplateExprRegExp {
                            in_char_class: false,
                            brace_depth,
                            js_ctx,
                        }
                    } else {
                        i += 1;
                        JsScanState::TemplateExpr {
                            brace_depth,
                            js_ctx,
                        }
                    }
                } else if bytes[i] == b'}' {
                    if brace_depth > 0 {
                        brace_depth -= 1;
                    }
                    i += 1;
                    if brace_depth == 0 {
                        JsScanState::TemplateLiteral
                    } else {
                        JsScanState::TemplateExpr {
                            brace_depth,
                            js_ctx,
                        }
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                }
            }
            JsScanState::TemplateExprSingleQuote {
                brace_depth,
                js_ctx,
            } => {
                if bytes[i] == b'\\' {
                    i += 2;
                    JsScanState::TemplateExprSingleQuote {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'\'' {
                    i += 1;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx: JsContext::DivOp,
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateExprSingleQuote {
                        brace_depth,
                        js_ctx,
                    }
                }
            }
            JsScanState::TemplateExprDoubleQuote {
                brace_depth,
                js_ctx,
            } => {
                if bytes[i] == b'\\' {
                    i += 2;
                    JsScanState::TemplateExprDoubleQuote {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'"' {
                    i += 1;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx: JsContext::DivOp,
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateExprDoubleQuote {
                        brace_depth,
                        js_ctx,
                    }
                }
            }
            JsScanState::TemplateExprRegExp {
                mut in_char_class,
                brace_depth,
                js_ctx,
            } => {
                if bytes[i] == b'\\' {
                    i += 2;
                    JsScanState::TemplateExprRegExp {
                        in_char_class,
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'[' {
                    in_char_class = true;
                    i += 1;
                    JsScanState::TemplateExprRegExp {
                        in_char_class,
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b']' {
                    in_char_class = false;
                    i += 1;
                    JsScanState::TemplateExprRegExp {
                        in_char_class,
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'/' && !in_char_class {
                    if is_script_tag_close_in_regexp(bytes, i) {
                        i += 1;
                        JsScanState::TemplateExprRegExp {
                            in_char_class,
                            brace_depth,
                            js_ctx,
                        }
                    } else {
                        i += 1;
                        JsScanState::TemplateExpr {
                            brace_depth,
                            js_ctx,
                        }
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateExprRegExp {
                        in_char_class,
                        brace_depth,
                        js_ctx,
                    }
                }
            }
            JsScanState::TemplateExprTemplateLiteral {
                brace_depth,
                js_ctx,
            } => {
                if bytes[i] == b'\\' {
                    i += 2;
                    JsScanState::TemplateExprTemplateLiteral {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == b'`' {
                    i += 1;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx: JsContext::DivOp,
                    }
                } else if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                    i += 2;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx: JsContext::DivOp,
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateExprTemplateLiteral {
                        brace_depth,
                        js_ctx,
                    }
                }
            }
            JsScanState::TemplateExprLineComment {
                brace_depth,
                js_ctx,
                preserve_body: _,
                keep_terminator: _,
            } => {
                if bytes[i] == b'\n' || bytes[i] == b'\r' {
                    i += 1;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                } else if bytes[i] == 0xE2
                    && i + 2 < bytes.len()
                    && ((bytes[i + 1] == 0x80 && bytes[i + 2] == 0xA8)
                        || (bytes[i + 1] == 0x80 && bytes[i + 2] == 0xA9))
                {
                    i += 3;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateExprLineComment {
                        brace_depth,
                        js_ctx,
                        preserve_body: true,
                        keep_terminator: true,
                    }
                }
            }
            JsScanState::TemplateExprBlockComment {
                brace_depth,
                js_ctx,
            } => {
                if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    i += 2;
                    JsScanState::TemplateExpr {
                        brace_depth,
                        js_ctx,
                    }
                } else {
                    i += 1;
                    JsScanState::TemplateExprBlockComment {
                        brace_depth,
                        js_ctx,
                    }
                }
            }
            JsScanState::LineComment {
                js_ctx,
                preserve_body: _,
                keep_terminator: _,
            } => {
                if bytes[i] == b'\n' || bytes[i] == b'\r' {
                    i += 1;
                    JsScanState::Expr { js_ctx }
                } else if i + 2 < bytes.len() && bytes[i] == 0xE2 {
                    let is_2028 = bytes[i + 1] == 0x80 && bytes[i + 2] == 0xA8;
                    if is_2028 || (bytes[i + 1] == 0x80 && bytes[i + 2] == 0xA9) {
                        i += 3;
                        JsScanState::Expr { js_ctx }
                    } else {
                        i += 1;
                        JsScanState::LineComment {
                            js_ctx,
                            preserve_body: true,
                            keep_terminator: true,
                        }
                    }
                } else {
                    i += 1;
                    JsScanState::LineComment {
                        js_ctx,
                        preserve_body: true,
                        keep_terminator: true,
                    }
                }
            }
            JsScanState::BlockComment { js_ctx } => {
                if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    i += 2;
                    JsScanState::Expr { js_ctx }
                } else {
                    i += 1;
                    JsScanState::BlockComment { js_ctx }
                }
            }
        };
    }

    None
}

fn find_style_close_tag(content: &str, start: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    if start >= bytes.len() {
        return None;
    }

    let mut state = CssScanState::Expr;
    let mut i = start;

    while i < bytes.len() {
        state = match state {
            CssScanState::Expr => {
                if is_html_close_tag(bytes, i, b"style") {
                    return Some(i);
                }

                match bytes[i] {
                    b'"' => {
                        i += 1;
                        CssScanState::DoubleQuote
                    }
                    b'\'' => {
                        i += 1;
                        CssScanState::SingleQuote
                    }
                    b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                        i += 2;
                        CssScanState::LineComment
                    }
                    b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                        i += 2;
                        CssScanState::BlockComment
                    }
                    _ => {
                        i += 1;
                        CssScanState::Expr
                    }
                }
            }
            CssScanState::SingleQuote => {
                if bytes[i] == b'\\' {
                    i += 2;
                    CssScanState::SingleQuote
                } else if bytes[i] == b'\'' {
                    i += 1;
                    CssScanState::Expr
                } else {
                    i += 1;
                    CssScanState::SingleQuote
                }
            }
            CssScanState::DoubleQuote => {
                if bytes[i] == b'\\' {
                    i += 2;
                    CssScanState::DoubleQuote
                } else if bytes[i] == b'"' {
                    i += 1;
                    CssScanState::Expr
                } else {
                    i += 1;
                    CssScanState::DoubleQuote
                }
            }
            CssScanState::LineComment => {
                if bytes[i] == b'\n' || bytes[i] == b'\r' || bytes[i] == b'\x0c' {
                    i += 1;
                    CssScanState::Expr
                } else if bytes[i] == 0xE2
                    && i + 2 < bytes.len()
                    && ((bytes[i + 1] == 0x80 && bytes[i + 2] == 0xA8)
                        || (bytes[i + 1] == 0x80 && bytes[i + 2] == 0xA9))
                {
                    i += 3;
                    CssScanState::Expr
                } else {
                    i += 1;
                    CssScanState::LineComment
                }
            }
            CssScanState::BlockComment => {
                if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    i += 2;
                    CssScanState::Expr
                } else {
                    i += 1;
                    CssScanState::BlockComment
                }
            }
        };
    }

    None
}

fn is_html_close_tag(bytes: &[u8], i: usize, tag: &[u8]) -> bool {
    if i + 2 + tag.len() > bytes.len() {
        return false;
    }
    if bytes[i] != b'<' || bytes[i + 1] != b'/' {
        return false;
    }
    if !bytes[i + 2..i + 2 + tag.len()]
        .iter()
        .zip(tag.iter())
        .all(|(lhs, rhs)| lhs.to_ascii_lowercase() == rhs.to_ascii_lowercase())
    {
        return false;
    }

    match bytes.get(i + 2 + tag.len()) {
        None => true,
        Some(&b' ') | Some(&b'\t') | Some(&b'\n') | Some(&b'\x0C') | Some(&b'\r') | Some(&b'/')
        | Some(&b'>') => true,
        Some(_) => false,
    }
}

fn is_script_tag_close_in_regexp(bytes: &[u8], i: usize) -> bool {
    if i == 0 || i + 7 > bytes.len() || bytes[i] != b'/' || bytes[i - 1] != b'<' {
        return false;
    }

    let tag = b"</script";
    bytes[i - 1..i + 7]
        .iter()
        .zip(tag.iter())
        .all(|(lhs, rhs)| lhs.to_ascii_lowercase() == rhs.to_ascii_lowercase())
}

fn filter_html_text_sections(prefix: &str, text: &str) -> String {
    let mut output = String::new();
    let mut section = if let Some(script_tag) = current_unclosed_script_tag(prefix) {
        if is_script_type_javascript(script_tag) {
            HtmlSection::Script
        } else {
            HtmlSection::Html
        }
    } else if current_unclosed_tag_content(prefix, "style").is_some() {
        HtmlSection::Style
    } else {
        HtmlSection::Html
    };

    let mut cursor = 0usize;
    let mut filtered_prefix = prefix.to_string();

    while cursor < text.len() {
        match section {
            HtmlSection::Html => {
                let next_script = find_open_tag(text, cursor, b"script");
                let next_style = find_open_tag(text, cursor, b"style");
                let (next, target) = match (next_script, next_style) {
                    (Some(a), Some(b)) if a <= b => (a, HtmlSection::Script),
                    (Some(_), Some(b)) => (b, HtmlSection::Style),
                    (Some(a), None) => (a, HtmlSection::Script),
                    (None, Some(b)) => (b, HtmlSection::Style),
                    (None, None) => (usize::MAX, HtmlSection::Html),
                };
                let target = if let HtmlSection::Script = target {
                    if let Some(end) = html_tag_end(text, next) {
                        if let Some(script_tag) = text.get(next..end) {
                            if is_script_type_javascript(script_tag) {
                                HtmlSection::Script
                            } else {
                                HtmlSection::Html
                            }
                        } else {
                            HtmlSection::Html
                        }
                    } else {
                        HtmlSection::Html
                    }
                } else {
                    target
                };

                if next == usize::MAX {
                    output.push_str(&text[cursor..]);
                    break;
                }

                let tag_end = match html_tag_end(text, next) {
                    Some(end) => end,
                    None => {
                        output.push_str(&text[next..]);
                        break;
                    }
                };

                output.push_str(&text[cursor..tag_end]);
                cursor = tag_end;
                filtered_prefix = format!("{}{}", prefix, output);
                section = target;
            }
            HtmlSection::Script => {
                let close = find_close_tag(text, cursor, b"script");
                if let Some(close_start) = close {
                    let segment = &text[cursor..close_start];
                    if !segment.is_empty() {
                        let filtered = filter_script_text(&filtered_prefix, segment);
                        output.push_str(&filtered);
                        filtered_prefix.push_str(&filtered);
                    }
                    let close_end = match html_tag_end(text, close_start) {
                        Some(end) => end,
                        None => {
                            output.push_str(&text[close_start..]);
                            break;
                        }
                    };
                    let close_tag = &text[close_start..close_end];
                    output.push_str(close_tag);
                    filtered_prefix.push_str(close_tag);
                    cursor = close_end;
                    section = HtmlSection::Html;
                } else {
                    let segment = &text[cursor..];
                    let filtered = filter_script_text(&filtered_prefix, segment);
                    output.push_str(&filtered);
                    filtered_prefix.push_str(&filtered);
                    break;
                }
            }
            HtmlSection::Style => {
                let close = find_close_tag(text, cursor, b"style");
                if let Some(close_start) = close {
                    output.push_str(&text[cursor..close_start]);
                    let close_end = match html_tag_end(text, close_start) {
                        Some(end) => end,
                        None => {
                            filtered_prefix.push_str(&text[close_start..]);
                            break;
                        }
                    };
                    let close_tag = &text[close_start..close_end];
                    output.push_str(close_tag);
                    filtered_prefix.push_str(close_tag);
                    cursor = close_end;
                    section = HtmlSection::Html;
                } else {
                    output.push_str(&text[cursor..]);
                    break;
                }
            }
        }
    }

    output
}

fn next_js_ctx(prefix: &str, preceding: JsContext) -> JsContext {
    let prefix = prefix.trim_end_matches(js_whitespace);
    if prefix.is_empty() {
        return preceding;
    }

    let bytes = prefix.as_bytes();
    let n = bytes.len();
    let c = bytes[n - 1];
    match c {
        b'+' | b'-' => {
            let mut start = n - 1;
            while start > 0 && bytes[start - 1] == c {
                start -= 1;
            }
            if (n - start) & 1 == 1 {
                JsContext::RegExp
            } else {
                JsContext::DivOp
            }
        }
        b'.' => {
            if n != 1 && bytes[n - 2].is_ascii_digit() {
                JsContext::DivOp
            } else {
                JsContext::RegExp
            }
        }
        b',' | b'<' | b'>' | b'=' | b'*' | b'%' | b'&' | b'|' | b'^' | b'?' | b'!' | b'~'
        | b'(' | b'[' | b':' | b';' | b'{' | b'}' => JsContext::RegExp,
        b'_' | b'/' | b'\\' => JsContext::DivOp,
        _ => {
            let mut j = n;
            while j > 0 && is_js_ident_part_byte(bytes[j - 1]) {
                j -= 1;
            }
            if is_js_ident_keyword(&prefix[j..]) {
                JsContext::RegExp
            } else {
                JsContext::DivOp
            }
        }
    }
}

fn is_js_ident_part_byte(byte: u8) -> bool {
    matches!(byte, b'$' | b'_' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z')
}

fn is_js_ident_keyword(keyword: &str) -> bool {
    matches!(
        keyword,
        "break"
            | "case"
            | "continue"
            | "delete"
            | "do"
            | "else"
            | "finally"
            | "in"
            | "instanceof"
            | "return"
            | "throw"
            | "try"
            | "typeof"
            | "void"
    )
}

fn js_whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\u{000C}'
            | '\u{000A}'
            | '\u{000D}'
            | '\u{0009}'
            | '\u{000B}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'
            | '\u{2001}'
            | '\u{2002}'
            | '\u{2003}'
            | '\u{2004}'
            | '\u{2005}'
            | '\u{2006}'
            | '\u{2007}'
            | '\u{2008}'
            | '\u{2009}'
            | '\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{FEFF}'
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CssScanState {
    Expr,
    SingleQuote,
    DoubleQuote,
    LineComment,
    BlockComment,
}

fn current_css_mode(content: &str) -> EscapeMode {
    match current_css_scan_state(content) {
        CssScanState::Expr => EscapeMode::StyleExpr,
        CssScanState::SingleQuote => EscapeMode::StyleString { quote: '\'' },
        CssScanState::DoubleQuote => EscapeMode::StyleString { quote: '"' },
        CssScanState::LineComment => EscapeMode::StyleLineComment,
        CssScanState::BlockComment => EscapeMode::StyleBlockComment,
    }
}

fn current_css_scan_state(content: &str) -> CssScanState {
    let bytes = content.as_bytes();
    let mut state = CssScanState::Expr;
    let mut i = 0usize;

    while i < bytes.len() {
        state = match state {
            CssScanState::Expr => {
                let ch = bytes[i];
                match ch {
                    b'"' => {
                        i += 1;
                        CssScanState::DoubleQuote
                    }
                    b'\'' => {
                        i += 1;
                        CssScanState::SingleQuote
                    }
                    b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                        i += 2;
                        CssScanState::LineComment
                    }
                    b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                        i += 2;
                        CssScanState::BlockComment
                    }
                    _ => {
                        i += 1;
                        CssScanState::Expr
                    }
                }
            }
            CssScanState::SingleQuote => {
                if bytes[i] == b'\\' {
                    i += 2;
                    CssScanState::SingleQuote
                } else if bytes[i] == b'\'' {
                    i += 1;
                    CssScanState::Expr
                } else {
                    i += 1;
                    CssScanState::SingleQuote
                }
            }
            CssScanState::DoubleQuote => {
                if bytes[i] == b'\\' {
                    i += 2;
                    CssScanState::DoubleQuote
                } else if bytes[i] == b'"' {
                    i += 1;
                    CssScanState::Expr
                } else {
                    i += 1;
                    CssScanState::DoubleQuote
                }
            }
            CssScanState::LineComment => {
                if bytes[i] == b'\n' || bytes[i] == b'\x0c' || bytes[i] == b'\r' {
                    i += 1;
                    CssScanState::Expr
                } else if bytes[i] == 0xE2
                    && i + 2 < bytes.len()
                    && ((bytes[i + 1] == 0x80 && bytes[i + 2] == 0xA8)
                        || (bytes[i + 1] == 0x80 && bytes[i + 2] == 0xA9))
                {
                    i += 3;
                    CssScanState::Expr
                } else {
                    i += 1;
                    CssScanState::LineComment
                }
            }
            CssScanState::BlockComment => {
                if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    i += 2;
                    CssScanState::Expr
                } else {
                    i += 1;
                    CssScanState::BlockComment
                }
            }
        };
    }

    state
}

fn escape_script_value(value: &Value) -> Result<String> {
    match value {
        Value::SafeHtml(raw) => Ok(raw.clone()),
        Value::SafeHtmlAttr(raw)
        | Value::SafeJs(raw)
        | Value::SafeCss(raw)
        | Value::SafeUrl(raw)
        | Value::SafeSrcset(raw) => {
            let encoded = serde_json::to_string(raw)?;
            Ok(sanitize_json_for_script(&encoded))
        }
        Value::Json(json) => {
            let encoded = serde_json::to_string(json)?;
            Ok(sanitize_json_for_script(&encoded))
        }
        Value::FunctionRef(name) => {
            let encoded = serde_json::to_string(&format!("<function:{name}>"))?;
            Ok(sanitize_json_for_script(&encoded))
        }
        Value::Missing => {
            let encoded = serde_json::to_string("<no value>")?;
            Ok(sanitize_json_for_script(&encoded))
        }
    }
}

fn sanitize_json_for_script(input: &str) -> String {
    input
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn normalize_url_for_attribute(input: &str) -> String {
    if !is_safe_url(input) {
        return "#ZgotmplZ".to_string();
    }
    encode_url_attribute_value(input)
}

fn is_safe_url(input: &str) -> bool {
    let trimmed = input.trim();

    if let Some((scheme, _remainder)) = trimmed.split_once(':')
        && !scheme.contains('/')
    {
        return scheme.eq_ignore_ascii_case("http")
            || scheme.eq_ignore_ascii_case("https")
            || scheme.eq_ignore_ascii_case("mailto");
    }

    true
}

fn encode_url_attribute_value(input: &str) -> String {
    let mut encoded = String::new();
    for &byte in input.as_bytes() {
        if is_safe_url_attr_byte(byte) {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(hex_upper((byte >> 4) & 0x0F));
            encoded.push(hex_upper(byte & 0x0F));
        }
    }
    encoded
}

fn filter_srcset_attribute_value(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::new();
    let mut start = 0usize;

    for i in 0..bytes.len() {
        if bytes[i] != b',' {
            continue;
        }

        filter_srcset_element(input, bytes, &mut start, i, &mut output);
        output.push(',');
        start = i + 1;
    }

    filter_srcset_element(input, bytes, &mut start, bytes.len(), &mut output);
    output
}

fn filter_srcset_element(
    input: &str,
    bytes: &[u8],
    start: &mut usize,
    end: usize,
    output: &mut String,
) {
    let mut left = *start;
    while left < end && is_html_space(bytes[left]) {
        left += 1;
    }

    let mut element_end = end;
    let mut i = left;
    while i < end {
        if is_html_space(bytes[i]) {
            element_end = i;
            break;
        }
        i += 1;
    }

    let url = &input[left..element_end];
    if !url.is_empty() && is_safe_url(url) && srcset_metadata_is_safe(&input[element_end..end]) {
        output.push_str(&input[*start..left]);
        output.push_str(&normalize_url_for_attribute(url).replace(',', "%2c"));
        output.push_str(&input[element_end..end]);
    } else {
        output.push_str("#ZgotmplZ");
    }

    *start = end;
}

fn srcset_metadata_is_safe(metadata: &str) -> bool {
    for byte in metadata.as_bytes() {
        if !is_html_space_or_ascii_alnum(*byte) {
            return false;
        }
    }
    true
}

fn is_html_space(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r' | b' ')
}

fn is_html_space_or_ascii_alnum(byte: u8) -> bool {
    is_html_space(byte) || (byte < 0x80 && byte.is_ascii_alphanumeric())
}

fn is_safe_url_attr_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'-' | b'.'
                | b'_'
                | b'~'
                | b':'
                | b'/'
                | b'?'
                | b'#'
                | b'['
                | b']'
                | b'@'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b'%'
        )
}

fn escape_js_string_fragment(input: &str, quote: char) -> String {
    let mut escaped = String::new();
    for ch in input.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\u{08}' => escaped.push_str("\\b"),
            '\u{0C}' => escaped.push_str("\\f"),
            '<' => escaped.push_str("\\x3C"),
            '>' => escaped.push_str("\\x3E"),
            '&' => escaped.push_str("\\x26"),
            '\u{2028}' => escaped.push_str("\\u2028"),
            '\u{2029}' => escaped.push_str("\\u2029"),
            c if quote == '`' && c == '`' => escaped.push_str("\\x60"),
            c if quote == '`' && c == '$' => {
                escaped.push('\\');
                escaped.push('$');
            }
            c if quote == '`' && c == '{' => {
                escaped.push('\\');
                escaped.push('{');
            }
            c if quote == '`' && c == '}' => {
                escaped.push('\\');
                escaped.push('}');
            }
            c if c == quote => {
                escaped.push('\\');
                escaped.push(c);
            }
            c if (c as u32) < 0x20 => {
                escaped.push_str(&format!("\\u{:04X}", c as u32));
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn escape_css_text(input: &str) -> String {
    let mut escaped = String::new();
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '-' | '_' | '.' | ',' | ':' | ';') {
            escaped.push(ch);
        } else {
            escaped.push_str(&format!("\\{:X} ", ch as u32));
        }
    }
    escaped
}

fn escape_css_string_fragment(input: &str, quote: char) -> String {
    let mut escaped = String::new();
    for ch in input.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\A "),
            '\r' => escaped.push_str("\\D "),
            '<' => escaped.push_str("\\3C "),
            '>' => escaped.push_str("\\3E "),
            c if c == quote => {
                escaped.push('\\');
                escaped.push(c);
            }
            c if (c as u32) < 0x20 => escaped.push_str(&format!("\\{:X} ", c as u32)),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn escape_attr_unquoted(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&#34;"),
            '\'' => escaped.push_str("&#39;"),
            '`' => escaped.push_str("&#96;"),
            '=' => escaped.push_str("&#61;"),
            '+' => escaped.push_str("&#43;"),
            ' ' => escaped.push_str("&#32;"),
            '\n' => escaped.push_str("&#10;"),
            '\r' => escaped.push_str("&#13;"),
            '\t' => escaped.push_str("&#9;"),
            '\0' => escaped.push_str("&#xfffd;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn percent_encode_url(input: &str) -> String {
    let mut encoded = String::new();
    for &byte in input.as_bytes() {
        if is_unreserved_url_byte(byte) {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(hex_upper((byte >> 4) & 0x0F));
            encoded.push(hex_upper(byte & 0x0F));
        }
    }
    encoded
}

fn is_unreserved_url_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

fn hex_upper(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'A' + (value - 10)) as char,
        _ => '0',
    }
}

fn escape_html(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&#34;"),
            '\'' => escaped.push_str("&#39;"),
            '+' => escaped.push_str("&#43;"),
            '\0' => escaped.push('\u{FFFD}'),
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    #[cfg(not(feature = "web-rust"))]
    use tempfile::tempdir;

    use super::*;

    fn test_func(_: &[Value]) -> Result<Value> {
        Ok(Value::from(0_i64))
    }

    fn test_method(_: &Value, _: &[Value]) -> Result<Value> {
        Ok(Value::from("ok"))
    }

    #[test]
    fn html_is_escaped_by_default() {
        let template = Template::new("page")
            .parse("<p>{{.Name}}</p>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Name": "<b>Alice</b>"}))
            .expect("execute should succeed");

        assert_eq!(output, "<p>&lt;b&gt;Alice&lt;/b&gt;</p>");
    }

    #[test]
    fn html_context_escapes_plus_nul_and_double_quote_go_compatibly() {
        let template = Template::new("page")
            .parse("<p>{{.Name}}</p>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Name": "Web + \"App\"\u{0000}"}))
            .expect("execute should succeed");

        assert_eq!(output, "<p>Web &#43; &#34;App&#34;\u{FFFD}</p>");
    }

    #[test]
    fn unquoted_attribute_context_escapes_plus_and_nul_go_compatibly() {
        let template = Template::new("attrs")
            .parse("<div data-v={{.Value}}></div>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Value": "A+\u{0000}B"}))
            .expect("execute should succeed");

        assert_eq!(output, "<div data-v=A&#43;&#xfffd;B></div>");
    }

    #[test]
    fn if_else_follows_go_like_truthiness() {
        let template = Template::new("if")
            .parse("{{if .Count}}has{{else}}empty{{end}}")
            .expect("parse should succeed");

        let output_false = template
            .execute_to_string(&json!({"Count": 0}))
            .expect("execute should succeed");
        let output_true = template
            .execute_to_string(&json!({"Count": 2}))
            .expect("execute should succeed");

        assert_eq!(output_false, "empty");
        assert_eq!(output_true, "has");
    }

    #[test]
    fn range_and_else_are_supported() {
        let template = Template::new("range")
            .parse("<ul>{{range .Items}}<li>{{.}}</li>{{else}}<li>none</li>{{end}}</ul>")
            .expect("parse should succeed");

        let output_items = template
            .execute_to_string(&json!({"Items": ["A", "<B>"]}))
            .expect("execute should succeed");
        let output_empty = template
            .execute_to_string(&json!({"Items": []}))
            .expect("execute should succeed");

        assert_eq!(output_items, "<ul><li>A</li><li>&lt;B&gt;</li></ul>");
        assert_eq!(output_empty, "<ul><li>none</li></ul>");
    }

    #[test]
    fn with_and_template_call_work() {
        let source = r#"
{{define "row"}}<tr><td>{{.}}</td></tr>{{end}}
<table>
{{with .Items}}{{range .}}{{template "row" .}}{{end}}{{else}}<tr><td>empty</td></tr>{{end}}
</table>
"#;
        let template = Template::new("table")
            .parse(source)
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Items": ["ok", "<ng>"]}))
            .expect("execute should succeed");

        let normalized = output.lines().map(str::trim).collect::<String>();
        assert!(normalized.contains("<tr><td>ok</td></tr>"));
        assert!(normalized.contains("<tr><td>&lt;ng&gt;</td></tr>"));
    }

    #[test]
    fn else_if_is_supported() {
        let template = Template::new("if")
            .parse("{{if .A}}a{{else if .B}}b{{else}}c{{end}}")
            .expect("parse should succeed");

        let out_a = template
            .execute_to_string(&json!({"A": true, "B": true}))
            .expect("execute should succeed");
        let out_b = template
            .execute_to_string(&json!({"A": false, "B": true}))
            .expect("execute should succeed");
        let out_c = template
            .execute_to_string(&json!({"A": false, "B": false}))
            .expect("execute should succeed");

        assert_eq!(out_a, "a");
        assert_eq!(out_b, "b");
        assert_eq!(out_c, "c");
    }

    #[test]
    fn functions_pipeline_and_safe_html_work() {
        let mut funcs = FuncMap::new();
        funcs.insert(
            "upper".to_string(),
            Arc::new(|args: &[Value]| {
                let arg = args.first().ok_or_else(|| {
                    TemplateError::Render("upper expects one argument".to_string())
                })?;
                Ok(Value::from(arg.to_plain_string().to_uppercase()))
            }) as Function,
        );

        let template = Template::new("funcs")
            .funcs(funcs)
            .parse("{{.Name | upper}} {{.Raw | safe_html}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Name": "alice", "Raw": "<span>x</span>"}))
            .expect("execute should succeed");

        assert_eq!(output, "ALICE <span>x</span>");
    }

    #[cfg(not(feature = "web-rust"))]
    #[test]
    fn parse_glob_loads_templates() {
        let dir = tempdir().expect("temp dir should be created");
        let list_path = dir.path().join("list.tmpl");
        let item_path = dir.path().join("item.tmpl");

        fs::write(
            &list_path,
            "<ul>{{range .Items}}{{template \"item\" .}}{{end}}</ul>",
        )
        .expect("list template should be written");
        fs::write(&item_path, "{{define \"item\"}}<li>{{.}}</li>{{end}}")
            .expect("item template should be written");

        let pattern = format!("{}/*.tmpl", dir.path().display());
        let template = Template::new("list.tmpl")
            .parse_glob(&pattern)
            .expect("parse_glob should succeed");

        let output = template
            .execute_template_to_string("list.tmpl", &json!({"Items": ["x", "<y>"]}))
            .expect("execute should succeed");

        assert_eq!(output, "<ul><li>x</li><li>&lt;y&gt;</li></ul>");
    }

    #[test]
    fn add_parse_tree_adds_named_template() {
        let tree = Template::new("base")
            .parse_tree("{{define \"greet\"}}Hello {{.Name}}{{end}}")
            .expect("parse_tree should succeed");

        let template = Template::new("base")
            .AddParseTree("greet", tree)
            .expect("AddParseTree should succeed");

        let output = template
            .execute_template_to_string("greet", &json!({"Name": "Alice"}))
            .expect("execute should succeed");

        assert_eq!(output, "Hello Alice");
    }

    #[test]
    fn add_parse_tree_overwrites_existing_definition() {
        let template = Template::new("base")
            .parse("{{define \"greet\"}}old {{.Name}}{{end}}")
            .expect("parse should succeed");
        let tree = Template::new("base")
            .parse_tree("{{define \"greet\"}}new {{.Name}}{{end}}")
            .expect("parse_tree should succeed");

        let template = template
            .AddParseTree("greet", tree)
            .expect("AddParseTree should overwrite existing template");

        let output = template
            .execute_template_to_string("greet", &json!({"Name": "alice"}))
            .expect("execute should succeed");

        assert_eq!(output, "new alice");
    }

    #[test]
    fn new_template_option_and_delims_are_shared() {
        let base = Template::new("base")
            .delims("[[", "]]")
            .add_func("upper", |args: &[Value]| {
                let value = args
                    .first()
                    .ok_or_else(|| TemplateError::Render("upper expects one arg".to_string()))?;
                Ok(Value::from(value.to_plain_string().to_uppercase()))
            })
            .option("missingkey=zero")
            .expect("option should succeed")
            .parse("[[define \"shared\"]]shared[[end]]")
            .expect("parse should succeed");

        let child = base
            .New("child")
            .parse("[[define \"only_child\"]][[upper .Name]]|[[.Missing]][[end]]");
        let child = child.expect("parse should succeed");

        let child_output = child
            .execute_template_to_string("only_child", &json!({ "Name": "alice" }))
            .expect("child execute should succeed");
        let base_output = base
            .execute_template_to_string("only_child", &json!({ "Name": "bob" }))
            .expect("base execute should succeed");

        assert_eq!(child_output, "ALICE|");
        assert_eq!(base_output, "BOB|");
    }

    #[test]
    fn new_shares_existing_template_namespace() {
        let base = Template::new("base")
            .add_func("upper", |args: &[Value]| {
                let value = args
                    .first()
                    .ok_or_else(|| TemplateError::Render("upper expects one arg".to_string()))?;
                Ok(Value::from(value.to_plain_string().to_uppercase()))
            })
            .parse("{{define \"shared\"}}{{upper .Name}}{{end}}")
            .expect("parse should succeed");

        let child = base
            .New("child")
            .parse("{{template \"shared\" .}}")
            .expect("parse should succeed");

        let child_output = child
            .execute_template_to_string("shared", &json!({"Name": "alice"}))
            .expect("child execute should succeed");
        let base_output = base
            .execute_template_to_string("shared", &json!({"Name": "bob"}))
            .expect("base execute should succeed");

        assert_eq!(child_output, "ALICE");
        assert_eq!(base_output, "BOB");
    }

    #[test]
    fn clone_creates_independent_namespace_after_execution() {
        let base = Template::new("base")
            .parse("{{.Name}}")
            .expect("parse should succeed");

        let cloned =
            base.Clone()
                .expect("clone should succeed")
                .add_func("upper", |args: &[Value]| {
                    let value = args.first().ok_or_else(|| {
                        TemplateError::Render("upper expects one arg".to_string())
                    })?;
                    Ok(Value::from(value.to_plain_string().to_uppercase()))
                });

        let _ = cloned
            .execute_to_string(&json!({ "Name": "cloned" }))
            .expect("execute should succeed");

        let base = base.parse("{{define \"only_base\"}}only-base{{end}}");
        let base = base.expect("parse on base should succeed");

        let base_output = base
            .execute_template_to_string("only_base", &json!({}))
            .expect("base execute should succeed");
        let cloned_output = cloned.execute_template_to_string("only_base", &json!({}));

        assert_eq!(base_output, "only-base");
        assert!(cloned_output.is_err());
    }

    #[test]
    fn clone_template_is_isolated_from_new_parses() {
        let base = Template::new("base")
            .parse("{{define \"shared\"}}shared{{end}}")
            .expect("parse should succeed");

        let cloned = base
            .Clone()
            .expect("clone should succeed")
            .parse("{{define \"clone-only\"}}clone-only{{end}}")
            .expect("parse on clone should succeed");

        assert!(base.lookup("clone-only").is_none());
        let output = cloned
            .execute_template_to_string("clone-only", &json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "clone-only");
    }

    #[test]
    fn clone_does_not_share_function_map() {
        let base = Template::new("base")
            .add_func("marker", |args: &[Value]| {
                let value = args
                    .first()
                    .ok_or_else(|| TemplateError::Render("marker expects one arg".to_string()))?;
                Ok(Value::from(format!("base:{}", value.to_plain_string())))
            })
            .parse("{{marker .Name}}")
            .expect("parse should succeed");

        let cloned =
            base.Clone()
                .expect("clone should succeed")
                .add_func("marker", |args: &[Value]| {
                    let value = args.first().ok_or_else(|| {
                        TemplateError::Render("marker expects one arg".to_string())
                    })?;
                    Ok(Value::from(format!("clone:{}", value.to_plain_string())))
                });

        let base_output = base
            .execute_to_string(&json!({ "Name": "Alice" }))
            .expect("execute should succeed");
        let cloned_output = cloned
            .execute_to_string(&json!({ "Name": "Alice" }))
            .expect("execute should succeed");

        assert_eq!(base_output, "base:Alice");
        assert_eq!(cloned_output, "clone:Alice");
    }

    #[cfg(not(feature = "web-rust"))]
    #[test]
    fn parse_files_requires_paths() {
        let err = match Template::new("page").parse_files(Vec::<&str>::new()) {
            Ok(_) => panic!("parse_files should fail without paths"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("parse_files requires at least one path")
        );

        let err = match parse_files::<Vec<&str>, &str>(Vec::new()) {
            Ok(_) => panic!("parse_files should fail without paths"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("parse_files requires at least one path")
        );
    }

    #[cfg(not(feature = "web-rust"))]
    #[test]
    fn parse_files_fails_for_missing_file() {
        let err = match Template::new("page").parse_files(vec!["does_not_exist.tmpl"]) {
            Ok(_) => panic!("parse_files should fail for missing file"),
            Err(err) => err,
        };
        assert!(
            err.to_string().contains("No such file")
                || err.to_string().contains("does_not_exist.tmpl")
        );
    }

    #[cfg(not(feature = "web-rust"))]
    #[test]
    fn parse_files_invalid_utf8_returns_invalid_utf8_error_code() {
        let dir = tempdir().expect("temp dir should be created");
        let template_path = dir.path().join("invalid.tmpl");
        fs::write(&template_path, &[0xffu8, 0xfeu8])
            .expect("invalid utf-8 template should be written");

        let template_path = template_path.to_string_lossy();
        let err = match Template::new("invalid").parse_files([template_path.as_ref()]) {
            Ok(_) => panic!("parse_files should fail for invalid utf-8 template"),
            Err(err) => err,
        };
        assert_eq!(err.code(), TemplateErrorCode::ErrInvalidUTF8);
    }

    #[cfg(not(feature = "web-rust"))]
    #[test]
    fn parse_glob_no_match_fails() {
        let err = match parse_glob("*.does_not_exist") {
            Ok(_) => panic!("parse_glob should fail when no matches"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("glob pattern matched no files"));

        let err = match Template::new("page").parse_fs(vec!["*.does_not_exist"]) {
            Ok(_) => panic!("parse_fs should fail when no matches"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("glob pattern matched no files"));
    }

    #[cfg(not(feature = "web-rust"))]
    #[test]
    fn parse_glob_rejects_invalid_pattern() {
        let err = match Template::new("page").parse_glob("[") {
            Ok(_) => panic!("parse_glob should reject invalid pattern"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("glob"));

        let err = match parse_glob("[") {
            Ok(_) => panic!("parse_glob should reject invalid pattern"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("glob"));
    }

    #[cfg(not(feature = "web-rust"))]
    #[test]
    fn parse_fs_accepts_multiple_patterns() {
        let dir = tempdir().expect("temp dir should be created");
        let first_path = dir.path().join("a.tmpl");
        let second_path = dir.path().join("b.tmpl");
        fs::write(&first_path, "first").expect("first template should be written");
        fs::write(&second_path, "{{define \"second\"}}second{{end}}")
            .expect("second template should be written");

        let first = first_path.to_string_lossy();
        let second = second_path.to_string_lossy();

        let template = Template::new("a.tmpl")
            .parse_fs([first.as_ref(), second.as_ref()])
            .expect("parse_fs should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "first");
    }

    #[cfg(not(feature = "web-rust"))]
    #[test]
    fn parse_fs_supports_glob_patterns() {
        let dir = tempdir().expect("temp dir should be created");
        let first_path = dir.path().join("file1.tmpl");
        let second_path = dir.path().join("file2.tmpl");
        fs::write(&first_path, "first").expect("first template should be written");
        fs::write(&second_path, "second").expect("second template should be written");

        let pattern = format!("{}/*.tmpl", dir.path().display());
        let template = Template::new("file1.tmpl")
            .parse_fs([pattern.as_str()])
            .expect("parse_fs should succeed with glob pattern");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "first");
    }

    #[cfg(not(feature = "web-rust"))]
    #[test]
    fn parse_fs_with_custom_filesystem() {
        #[derive(Clone)]
        struct MemoryFS {
            files: std::collections::HashMap<String, Vec<u8>>,
        }

        impl TemplateFS for MemoryFS {
            fn read_file(&self, path: &str) -> Result<Vec<u8>> {
                self.files
                    .get(path)
                    .cloned()
                    .ok_or_else(|| TemplateError::Parse(format!("file not found: {path}")))
            }

            fn glob(&self, pattern: &str) -> Result<Vec<String>> {
                if pattern == "*" {
                    Ok(self.files.keys().cloned().collect())
                } else if let Some(path) = self.files.get(pattern).map(|_| pattern.to_string()) {
                    Ok(vec![path])
                } else {
                    Ok(Vec::new())
                }
            }
        }

        let filesystem = MemoryFS {
            files: std::collections::HashMap::from([
                (
                    "base.tmpl".to_string(),
                    b"base {{template \"partial\" .}}".to_vec(),
                ),
                (
                    "partial.tmpl".to_string(),
                    b"{{define \"partial\"}}partial{{end}}".to_vec(),
                ),
            ]),
        };

        let template = Template::new("base.tmpl")
            .ParseFS(&filesystem, ["base.tmpl", "partial.tmpl"])
            .expect("ParseFS should parse from custom filesystem");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "base partial");
        assert_eq!(
            template.DefinedTemplates(),
            "; defined templates are: base.tmpl, partial, partial.tmpl"
        );
    }

    #[cfg(feature = "web-rust")]
    #[test]
    fn parse_files_is_not_supported_in_web_rust() {
        let err = match Template::new("page").parse_files(vec!["template.tmpl"]) {
            Ok(_) => panic!("parse_files should not be supported"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("parse_files is not supported in web-rust builds")
        );

        let err = match parse_files(vec!["template.tmpl"]) {
            Ok(_) => panic!("parse_files should not be supported"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("parse_files is not supported in web-rust builds")
        );
    }

    #[cfg(feature = "web-rust")]
    #[test]
    fn parse_glob_is_not_supported_in_web_rust() {
        let err = match Template::new("page").parse_glob("*.tmpl") {
            Ok(_) => panic!("parse_glob should not be supported"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("parse_glob is not supported in web-rust builds")
        );

        let err = match parse_glob("*.tmpl") {
            Ok(_) => panic!("parse_glob should not be supported"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("parse_glob is not supported in web-rust builds")
        );
    }

    #[cfg(feature = "web-rust")]
    #[test]
    fn parse_fs_is_not_supported_in_web_rust() {
        let err = match Template::new("page").parse_fs(vec!["*.tmpl"]) {
            Ok(_) => panic!("parse_fs should not be supported"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("parse_fs is not supported in web-rust builds")
        );

        let err = match parse_fs(vec!["*.tmpl"]) {
            Ok(_) => panic!("parse_fs should not be supported"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("parse_fs is not supported in web-rust builds")
        );
    }

    #[test]
    fn variable_declaration_assignment_and_lookup_work() {
        let template = Template::new("vars")
            .parse("{{$x := .Name}}{{$x}}/{{$x = \"bob\"}}{{$x}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Name": "<alice>"}))
            .expect("execute should succeed");

        assert_eq!(output, "&lt;alice&gt;/bob");
    }

    #[test]
    fn variables_are_scoped_to_their_control_block() {
        let template = Template::new("scope")
            .parse("{{$x := \"root\"}}{{if true}}{{$x := \"inner\"}}{{end}}{{$x}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "root");
    }

    #[test]
    fn range_supports_go_style_variable_declaration() {
        let template = Template::new("range")
            .parse("{{range $i, $v := .Items}}{{$i}}={{$v}};{{end}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Items": ["a", "<b>"]}))
            .expect("execute should succeed");

        assert_eq!(output, "0=a;1=&lt;b&gt;;");
    }

    #[test]
    fn template_call_does_not_inherit_variables() {
        let template = Template::new("root")
            .parse("{{$x := \"root\"}}{{define \"child\"}}{{$x}}{{end}}{{template \"child\" .}}")
            .expect("parse should succeed");

        let error = template
            .execute_to_string(&json!({}))
            .expect_err("execute should fail");

        assert!(error.to_string().contains("variable `$x`"));
    }

    #[test]
    fn block_supports_default_and_override_template() {
        let default_template = Template::new("page")
            .parse(
                "{{define \"base\"}}<body>{{block \"content\" .}}Default{{end}}</body>{{end}}{{template \"base\" .}}",
            )
            .expect("parse should succeed");
        let default_output = default_template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        assert_eq!(default_output, "<body>Default</body>");

        let overridden_template = Template::new("page")
            .parse(
                "{{define \"content\"}}<p>{{.Msg}}</p>{{end}}{{define \"base\"}}<body>{{block \"content\" .}}Default{{end}}</body>{{end}}{{template \"base\" .}}",
            )
            .expect("parse should succeed");
        let overridden_output = overridden_template
            .execute_to_string(&json!({"Msg": "Hi"}))
            .expect("execute should succeed");
        assert_eq!(overridden_output, "<body><p>Hi</p></body>");
    }

    #[test]
    fn index_and_comparison_functions_work() {
        let template = Template::new("funcs")
            .parse("{{index .Items 1}}|{{index .Map \"k\"}}|{{index .Nested \"a\" 0}}|{{lt .A .B}}/{{ge .B .A}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({
                "Items": ["x", "<y>"],
                "Map": {"k": "v"},
                "Nested": {"a": ["<z>"]},
                "A": 1,
                "B": 2
            }))
            .expect("execute should succeed");

        assert_eq!(output, "&lt;y&gt;|v|&lt;z&gt;|true/true");
    }

    #[test]
    fn attribute_context_uses_attribute_escaping() {
        let template = Template::new("attr")
            .parse("<div data-v={{.Value}} title=\"{{.Title}}\"></div>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Value": "a b=<x>", "Title": "\"q\" & <t>"}))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<div data-v=a&#32;b&#61;&lt;x&gt; title=\"&#34;q&#34; &amp; &lt;t&gt;\"></div>"
        );
    }

    #[test]
    fn url_attribute_context_percent_encodes_values() {
        let template = Template::new("url")
            .parse("<a href=\"{{.URL}}\">go</a>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"URL": "https://example.com/q?a=b c&x=<y>"}))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<a href=\"https://example.com/q?a=b%20c&amp;x=%3Cy%3E\">go</a>"
        );
    }

    #[test]
    fn url_attribute_blocks_javascript_scheme() {
        let template = Template::new("url")
            .parse("<a href=\"{{.URL}}\">go</a>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"URL": "javascript:alert(1)"}))
            .expect("execute should succeed");

        assert_eq!(output, "<a href=\"#ZgotmplZ\">go</a>");
    }

    #[test]
    fn url_attribute_blocks_non_http_https_mailto_schemes() {
        let template = Template::new("url")
            .parse("<a href=\"{{.URL}}\">go</a>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"URL": "tel:+1-212-555-1212"}))
            .expect("execute should succeed");

        assert_eq!(output, "<a href=\"#ZgotmplZ\">go</a>");
    }

    #[test]
    fn url_attribute_allows_mailto_scheme() {
        let template = Template::new("url")
            .parse("<a href=\"{{.URL}}\">go</a>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"URL": "mailto:alice@example.com"}))
            .expect("execute should succeed");

        assert_eq!(output, "<a href=\"mailto:alice@example.com\">go</a>");
    }

    #[test]
    fn namespaced_and_data_attributes_follow_go_like_context_rules() {
        let template = Template::new("attrs")
            .parse(
                "<a my:href=\"{{.A}}\" data-href=\"{{.B}}\" my:data-href=\"{{.C}}\" xmlns:onclick=\"{{.D}}\"></a>",
            )
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({
                "A": "javascript:alert(1)",
                "B": "javascript:alert(2)",
                "C": "javascript:alert(3)",
                "D": "javascript:alert(4)"
            }))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<a my:href=\"#ZgotmplZ\" data-href=\"#ZgotmplZ\" my:data-href=\"javascript:alert(3)\" xmlns:onclick=\"#ZgotmplZ\"></a>"
        );
    }

    #[test]
    fn dynamic_attribute_name_with_static_prefix_is_merged() {
        let template = Template::new("attrs")
            .parse("<img on{{.Suffix}}=\"alert({{.Msg}})\">")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Suffix": "load", "Msg": "loaded"}))
            .expect("execute should succeed");

        assert_eq!(output, "<img onload=\"alert(&#34;loaded&#34;)\">");
    }

    #[test]
    fn dynamic_attribute_name_bad_event_handler_is_rejected() {
        let template = Template::new("attrs")
            .parse("<input {{.Name}}=\"{{.Value}}\">")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Name": "onchange", "Value": "doEvil()"}))
            .expect("execute should succeed");

        assert_eq!(output, "<input #ZgotmplZ=\"doEvil()\">");
    }

    #[test]
    fn dynamic_attribute_name_bad_css_handler_is_rejected() {
        let template = Template::new("attrs")
            .parse("<div {{.Name}}=\"{{.Value}}\"></div>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Name": "style", "Value": "color: expression(alert(1337))"}))
            .expect("execute to succeed");

        assert_eq!(
            output,
            "<div #ZgotmplZ=\"color: expression(alert(1337))\"></div>"
        );
    }

    #[test]
    fn dynamic_attribute_name_bad_url_handler_is_rejected() {
        let template = Template::new("attrs")
            .parse("<img {{.Name}}=\"{{.Value}}\">")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Name": "src", "Value": "javascript:alert(1)"}))
            .expect("execute should succeed");

        assert_eq!(output, "<img #ZgotmplZ=\"javascript:alert(1)\">");
    }

    #[test]
    fn style_attribute_escapes_template_call_values() {
        let template = Template::new("attrs")
            .parse(
                "{{define \"injected\"}}{{.}}{{end}}<a style=\"{{template \"injected\" .A}}\"></a>",
            )
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "</script>"}))
            .expect("execute to succeed");

        assert_eq!(output, "<a style=\"\\3C \\2F script\\3E \"></a>");
    }

    #[test]
    fn dynamic_attribute_name_empty_value_is_rejected() {
        let template = Template::new("attrs")
            .parse("<input {{.Name}} name=n>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Name": ""}))
            .expect("parse should succeed");

        assert_eq!(output, "<input #ZgotmplZ name=n>");
    }

    #[test]
    fn attr_type_map_plain_attribute_is_escaped_but_not_url_normalized() {
        let template = Template::new("attrs")
            .parse("<input accept-charset=\"{{.Value}}\">")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Value": "javascript:alert(1)"}))
            .expect("execute should succeed");

        assert_eq!(output, "<input accept-charset=\"javascript:alert(1)\">");
    }

    #[test]
    fn attr_type_map_url_attribute_and_heuristics_are_applied() {
        let template = Template::new("attrs")
            .parse("<a action=\"{{.Value}}\" custom-src=\"{{.Value2}}\"></a>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({
                "Value": "javascript:alert(1)",
                "Value2": "javascript:alert(2)"
            }))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<a action=\"#ZgotmplZ\" custom-src=\"#ZgotmplZ\"></a>"
        );
    }

    #[test]
    fn srcset_attribute_filters_each_element() {
        let template = Template::new("srcset")
            .parse("<img srcset=\"{{.Srcset}}\">")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Srcset": " /foo/bar.png 200w, /baz/boo(1).png"}))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<img srcset=\" /foo/bar.png 200w, /baz/boo(1).png\">"
        );
    }

    #[test]
    fn srcset_attribute_rejects_unsafe_elements() {
        let template = Template::new("srcset")
            .parse("<img srcset=\"{{.Srcset}}\">")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Srcset": "javascript:alert(1), /foo.png"}))
            .expect("execute should succeed");

        assert_eq!(output, "<img srcset=\"#ZgotmplZ, /foo.png\">");
    }

    #[test]
    fn srcset_attribute_rejects_unsafe_metadata() {
        let template = Template::new("srcset")
            .parse("<img srcset=\"{{.Srcset}}\">")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Srcset": "/bogus#, javascript:alert(1)"}))
            .expect("execute should succeed");

        assert_eq!(output, "<img srcset=\"/bogus#,#ZgotmplZ\">");
    }

    #[test]
    fn html_comments_in_template_source_are_stripped() {
        let template = Template::new("comments")
            .parse("<div>a<!--hidden-->{{.X}}<!--{{.Y}}--></div>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"X": "<b>", "Y": "ignored"}))
            .expect("execute should succeed");

        assert_eq!(output, "<div>a&lt;b&gt;</div>");
    }

    #[test]
    fn script_context_emits_json_escaped_literals() {
        let template = Template::new("script")
            .parse("<script>const x = {{.X}}; const y = {{.Y}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"X": "<tag>", "Y": {"a": "<b>"}}))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<script>const x = \"\\u003ctag\\u003e\"; const y = {\"a\":\"\\u003cb\\u003e\"};</script>"
        );
    }

    #[test]
    fn script_context_handles_utf8_and_unicode_line_separators() {
        let template = Template::new("script")
            .parse("<script>const s = \"あ\";\u{2028}const t = \"い\";\u{2029}const x = {{.X}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"X": "<tag>"}))
            .expect("execute should succeed");

        assert!(output.contains("\u{2028}"));
        assert!(output.contains("\u{2029}"));
        assert!(output.contains("const x = \"\\u003ctag\\u003e\";"));
    }

    #[test]
    fn script_type_template_is_not_treated_as_javascript() {
        let template = Template::new("script")
            .parse("<script type=\"text/template\">{{.X}}</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"X": "<tag>"}))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<script type=\"text/template\">&lt;tag&gt;</script>"
        );
    }

    #[test]
    fn script_type_module_is_treated_as_javascript() {
        let template = Template::new("script")
            .parse("<script type='module'>const x = {{.X}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"X": "<tag>"}))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<script type='module'>const x = \"\\u003ctag\\u003e\";</script>"
        );
    }

    #[test]
    fn script_type_json_serializes_as_json() {
        let template = Template::new("script")
            .parse("<script type=\"application/ld+json\">{{.}}</script>")
            .expect("parse should succeed");

        const PREFIX: &str = "<script type=\"application/ld+json\">";
        const SUFFIX: &str = "</script>";
        let tests = [
            "",
            "\u{FFFD}",
            "\u{0000}",
            "\u{001F}",
            "\t",
            "<>",
            "\"'",
            "ASCII letters",
            "ʕ⊙ϖ⊙ʔ",
            "🍕",
        ];

        for input in tests {
            let rendered = template
                .execute_to_string(&input)
                .expect("execute should succeed");
            let json_text = rendered
                .strip_prefix(PREFIX)
                .and_then(|value| value.strip_suffix(SUFFIX))
                .expect("rendered script wrapper should match expected format");
            let output: String =
                serde_json::from_str(json_text).expect("script contents should be valid JSON");
            assert_eq!(output, input);
        }
    }

    #[test]
    fn script_template_context_escapes_template_literal_tokens() {
        let template = Template::new("script")
            .parse("<script>const s = `{{.S}}`;</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"S": "a`b${c}"}))
            .expect("execute should succeed");

        assert_eq!(output, "<script>const s = `a\\x60b\\$\\{c\\}`;</script>");
    }

    #[test]
    fn script_template_context_escapes_template_injected_values() {
        let template = Template::new("script")
            .parse(
                "{{define \"injected\"}}{{.}}{{end}}<script>const s = `{{template \"injected\" .R}}`;</script>",
            )
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"R": "a</script>${x}"}))
            .expect("execute to succeed");

        assert_eq!(
            output,
            "<script>const s = `a\\x3C/script\\x3E\\$\\{x\\}`;</script>"
        );
    }

    #[test]
    fn script_template_context_keeps_script_state_with_mixed_case_close_tag() {
        let template = Template::new("script")
            .parse("<script>const s = `</SCRIPT>`; const x = {{.X}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"X": "<x>"}))
            .expect("execute to succeed");

        assert_eq!(
            output,
            "<script>const s = `</SCRIPT>`; const x = \"\\u003cx\\u003e\";</script>"
        );
    }

    #[test]
    fn script_context_keeps_script_state_for_literals_with_close_tag() {
        let template = Template::new("script")
            .parse("<script>const s = \"</script>\"; const x = {{.X}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"X": "<x>"}))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<script>const s = \"</script>\"; const x = \"\\u003cx\\u003e\";</script>"
        );
    }

    #[test]
    fn script_regexp_context_escapes_regexp_delimiters() {
        let template = Template::new("script")
            .parse("<script>const r = /{{.R}}/i;</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"R": "a/b</script>"}))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<script>const r = /a\\/b\\x3C\\/script\\x3E/i;</script>"
        );
    }

    #[test]
    fn script_regexp_context_escapes_template_injected_values() {
        let template = Template::new("script")
            .parse(
                "{{define \"injected\"}}{{.}}{{end}}<script>const r = /{{template \"injected\" .R}}/i;</script>",
            )
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"R": "a/b</sCrIpT>"}))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<script>const r = /a\\/b\\x3C\\/sCrIpT\\x3E/i;</script>"
        );
    }

    #[test]
    fn script_regexp_context_reverts_to_expr_after_template() {
        let template = Template::new("script")
            .parse("<script>const r = /{{.R}}/; const x = {{.X}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"R": "a/b</sCrIpT>", "X": "<x>"}))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<script>const r = /a\\/b\\x3C\\/sCrIpT\\x3E/; const x = \"\\u003cx\\u003e\";</script>"
        );
    }

    #[test]
    fn script_regexp_context_ignores_fake_close_inside_pattern() {
        let rendered = "<script>const r = /a</script> b/; const x = 1;</script>";
        let start = rendered
            .find('>')
            .map(|idx| idx + 1)
            .expect("script tag should have close >");
        let fake_close = rendered[..start].find("</script>");
        let actual_close = find_script_close_tag(rendered, start).unwrap();
        let expected_close = rendered.rfind("</script>").unwrap();

        assert!(fake_close.is_none());
        assert_eq!(actual_close, expected_close);
        assert!(current_unclosed_tag_content(rendered, "script").is_none());
    }

    #[test]
    fn script_template_context_reverts_to_expr_after_template() {
        let template = Template::new("script")
            .parse("<script>const s = `{{.S}}`; const x = {{.X}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"S": "</script>${x}", "X": "<x>"}))
            .expect("execute to succeed");

        assert_eq!(
            output,
            "<script>const s = `\\x3C/script\\x3E\\$\\{x\\}`; const x = \"\\u003cx\\u003e\";</script>"
        );
    }

    #[test]
    fn script_template_expr_context_ignores_template_inner_quotes() {
        let template = Template::new("script")
            .parse(r#"<script>const s = `a ${"b}c" + {{.R}}}`;</script>"#)
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"R": "<x>"}))
            .expect("execute should succeed");

        assert_eq!(
            output,
            r#"<script>const s = `a ${"b}c" + "\u003cx\u003e"}`;</script>"#
        );
    }

    #[test]
    fn debug_script_template_expr_inner_case_close_tag_detection() {
        let rendered = r#"<script>const s = `a ${"b}c" + 0}`;</script>"#;
        let start = rendered
            .find('>')
            .map(|idx| idx + 1)
            .expect("script tag should have close >");
        let close = find_script_close_tag(rendered, start);
        let unclosed = current_unclosed_tag_content(rendered, "script");
        println!("quotes close = {:?}", close);
        println!(
            "quotes unclosed is_some={} suffix_len={}",
            unclosed.is_some(),
            unclosed.map(|s| s.len()).unwrap_or_default()
        );
        if let Some(content) = unclosed {
            println!(
                "quotes unclosed tail state = {:?}",
                current_js_scan_state(content)
            );
        }

        let alt = "<script>const s = `a ${\"x\" // }\n+ 0}`;</script>";
        let alt_start = alt
            .find('>')
            .map(|idx| idx + 1)
            .expect("script tag should have close >");
        let alt_close = find_script_close_tag(alt, alt_start);
        let alt_unclosed = current_unclosed_tag_content(alt, "script");
        println!("line close = {:?}", alt_close);
        println!(
            "line unclosed is_some={} suffix_len={}",
            alt_unclosed.is_some(),
            alt_unclosed.map(|s| s.len()).unwrap_or_default()
        );
        if let Some(content) = alt_unclosed {
            println!(
                "line unclosed tail state = {:?}",
                current_js_scan_state(content)
            );
        }

        let commented = "<script>const s = `a ${\"x\" /* } */ + 0}`;</script>";
        let commented_start = commented
            .find('>')
            .map(|idx| idx + 1)
            .expect("script tag should have close >");
        let commented_close = find_script_close_tag(commented, commented_start);
        let commented_unclosed = current_unclosed_tag_content(commented, "script");
        println!("block close = {:?}", commented_close);
        println!(
            "block unclosed is_some={} suffix_len={}",
            commented_unclosed.is_some(),
            commented_unclosed.map(|s| s.len()).unwrap_or_default()
        );
        if let Some(content) = commented_unclosed {
            println!(
                "block unclosed tail state = {:?}",
                current_js_scan_state(content)
            );
        }

        let raw_quotes = r#"<script>const s = `a ${"b}c" + {{.R}}}`;</script>"#;
        let mut template = Template::new("script");
        template
            .parse_named("script", raw_quotes)
            .expect("parse_named should succeed");
        let raw_templates = template.name_space.templates.read().unwrap().clone();
        if let Some(nodes) = raw_templates.get("script") {
            println!("quotes nodes = {:#?}", nodes);
            let mut tracker = ContextTracker::from_state(ContextState::html_text());
            for node in nodes {
                match node {
                    Node::Text(text) => {
                        let before = tracker.state().mode;
                        tracker.append_text(text);
                        println!(
                            "quotes raw text before={before:?} after={:?}",
                            tracker.state().mode
                        );
                    }
                    Node::Expr { .. } => {
                        let mode = tracker.mode();
                        println!("quotes raw expr mode={mode:?}");
                        tracker.append_expr_placeholder(mode);
                    }
                    _ => {}
                }
            }
            println!("quotes raw final state = {:?}", tracker.state());
            let mut nodes_for_analysis = nodes.clone();
            let mut analyze = ParseContextAnalyzer::new(raw_templates.clone());
            let mut stepped_tracker = ContextTracker::from_state(ContextState::html_text());
            for node in &mut nodes_for_analysis {
                let flows = analyze
                    .analyze_node(node, stepped_tracker.clone(), false)
                    .expect("analyze node should succeed");
                let next_state = flows
                    .iter()
                    .map(|flow| flow.tracker.state())
                    .collect::<Vec<_>>();
                println!("quotes analyzed node step state after {:?}", next_state);
                println!(
                    "quotes analyzed node step rendered {:?}",
                    flows
                        .iter()
                        .map(|flow| flow.tracker.rendered.as_str())
                        .collect::<Vec<_>>()
                );
                stepped_tracker = flows
                    .into_iter()
                    .next()
                    .expect("node should produce one flow")
                    .tracker;
            }
            println!(
                "quotes stepped tracker state = {:?}",
                stepped_tracker.state()
            );
            let analyzed_flows = ParseContextAnalyzer::new(raw_templates.clone())
                .analyze_nodes(
                    &mut nodes_for_analysis,
                    ContextTracker::from_state(ContextState::html_text()),
                    false,
                )
                .expect("analyze_nodes should succeed");
            println!(
                "quotes direct analyze_nodes states: {:?}",
                analyzed_flows
                    .iter()
                    .map(|flow| (flow.kind.clone(), flow.tracker.state()))
                    .collect::<Vec<_>>()
            );
        }
        let mut analyzer = ParseContextAnalyzer::new(raw_templates);
        let quotes_end = analyzer
            .analyze_template("script", ContextState::html_text())
            .expect("analysis should succeed");
        println!("quotes analyzer end state = {:?}", quotes_end);

        let raw_line = "<script>const s = `a ${\"x\" // }\n+ {{.R}}}`;</script>";
        let mut template = Template::new("script");
        template
            .parse_named("script", raw_line)
            .expect("parse_named should succeed");
        let raw_templates = template.name_space.templates.read().unwrap().clone();
        if let Some(nodes) = raw_templates.get("script") {
            println!("line nodes = {:?}", nodes);
            let mut nodes_for_analysis = nodes.clone();
            let mut tracker = ContextTracker::from_state(ContextState::html_text());
            let mut analyzer = ParseContextAnalyzer::new(raw_templates.clone());
            for node in nodes_for_analysis.iter_mut() {
                let flows = analyzer
                    .analyze_node(node, tracker.clone(), false)
                    .expect("line analyze node should succeed");
                println!(
                    "line analyzed node step state after {:?}",
                    flows
                        .iter()
                        .map(|flow| flow.tracker.state())
                        .collect::<Vec<_>>()
                );
                tracker = flows
                    .into_iter()
                    .next()
                    .expect("line node should produce one flow")
                    .tracker;
            }
            println!("line manual final state = {:?}", tracker.state());
        }
        let mut analyzer = ParseContextAnalyzer::new(raw_templates);
        let line_end = analyzer
            .analyze_template("script", ContextState::html_text())
            .expect("analysis should succeed");
        println!("line analyzer end state = {:?}", line_end);

        let raw_block = "<script>const s = `a ${\"x\" /* } */ + {{.R}}}`;</script>";
        let mut template = Template::new("script");
        template
            .parse_named("script", raw_block)
            .expect("parse_named should succeed");
        let raw_templates = template.name_space.templates.read().unwrap().clone();
        if let Some(nodes) = raw_templates.get("script") {
            println!("block nodes = {:?}", nodes);
            let mut nodes_for_analysis = nodes.clone();
            let mut tracker = ContextTracker::from_state(ContextState::html_text());
            let mut analyzer = ParseContextAnalyzer::new(raw_templates.clone());
            for node in nodes_for_analysis.iter_mut() {
                let flows = analyzer
                    .analyze_node(node, tracker.clone(), false)
                    .expect("block analyze node should succeed");
                println!(
                    "block analyzed node step state after {:?}",
                    flows
                        .iter()
                        .map(|flow| flow.tracker.state())
                        .collect::<Vec<_>>()
                );
                tracker = flows
                    .into_iter()
                    .next()
                    .expect("block node should produce one flow")
                    .tracker;
            }
            println!("block manual final state = {:?}", tracker.state());
        }
        let mut analyzer = ParseContextAnalyzer::new(raw_templates);
        let block_end = analyzer
            .analyze_template("script", ContextState::html_text())
            .expect("analysis should succeed");
        println!("block analyzer end state = {:?}", block_end);
    }

    fn analyze_expr_modes(name: &str, source: &str) -> Vec<EscapeMode> {
        let template = Template::new(name)
            .parse(source)
            .expect("parse should succeed");

        let raw_templates = template.name_space.templates.read().unwrap().clone();
        let mut nodes = raw_templates
            .get(name)
            .expect("template should exist")
            .clone();

        let mut analyzer = ParseContextAnalyzer::new(raw_templates);
        let mut tracker = ContextTracker::from_state(ContextState::html_text());
        for node in nodes.iter_mut() {
            let mut flows = analyzer
                .analyze_node(node, tracker, false)
                .expect("analyze_node should succeed");
            assert_eq!(flows.len(), 1);
            tracker = flows.pop().expect("one flow should be produced").tracker;
        }

        nodes
            .into_iter()
            .filter_map(|node| match node {
                Node::Expr { mode, .. } => Some(mode),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn parse_context_tracks_script_template_and_regexp_states() {
        let template_states = analyze_expr_modes(
            "script_template_state",
            "<script>const s = `{{.S}}`;</script>",
        );
        assert_eq!(template_states, vec![EscapeMode::ScriptTemplate]);

        let regexp_states = analyze_expr_modes(
            "script_regexp_state",
            "<script>const r = /{{.R}}/i;</script>",
        );
        assert_eq!(regexp_states, vec![EscapeMode::ScriptRegexp]);
    }

    #[test]
    fn parse_context_tracks_script_comment_states() {
        let line_states = analyze_expr_modes(
            "script_comment_state_line",
            "<script>// {{.A}}\nconst x = {{.B}};</script>",
        );
        assert_eq!(
            line_states,
            vec![EscapeMode::ScriptLineComment, EscapeMode::ScriptExpr]
        );

        let block_states = analyze_expr_modes(
            "script_comment_state_block",
            "<script>/* {{.A}} */const x = {{.B}};</script>",
        );
        assert_eq!(
            block_states,
            vec![EscapeMode::ScriptBlockComment, EscapeMode::ScriptExpr]
        );
    }

    #[test]
    fn parse_context_tracks_style_comment_states() {
        let block_states = analyze_expr_modes(
            "style_comment_state",
            "<style>/* {{.A}} */.a { color: {{.B}}; }</style>",
        );
        assert_eq!(
            block_states,
            vec![EscapeMode::StyleBlockComment, EscapeMode::StyleExpr]
        );
    }

    #[test]
    fn script_template_expr_context_ignores_template_inner_line_comment_text() {
        let template = Template::new("script")
            .parse("<script>const s = `a ${\"x\" // }\n+ {{.R}}}`;</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"R": "<x>"}))
            .expect("execute to succeed");

        assert_eq!(
            output,
            "<script>const s = `a ${\"x\" \n+ \"\\u003cx\\u003e\"}`;</script>"
        );
    }

    #[test]
    fn script_template_expr_context_ignores_template_inner_block_comment_text() {
        let template = Template::new("script")
            .parse("<script>const s = `a ${\"x\" /* } */ + {{.R}}}`;</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"R": "<x>"}))
            .expect("execute to succeed");

        assert_eq!(
            output,
            "<script>const s = `a ${\"x\"   + \"\\u003cx\\u003e\"}`;</script>"
        );
    }

    #[test]
    fn script_template_expr_context_ignores_regexp_curly_in_expr() {
        let template = Template::new("script")
            .parse("<script>const s = `${/\\}/.test(\"x\") ? {{.R}} : 0}`;</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"R": 1}))
            .expect("execute to succeed");

        assert_eq!(
            output,
            "<script>const s = `${/\\}/.test(\"x\") ? 1 : 0}`;</script>"
        );
    }

    #[test]
    fn script_line_comment_mode_ignores_insertions() {
        let template = Template::new("script")
            .parse("<script>// {{.A}}\nconst x = {{.B}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "inject", "B": 1}))
            .expect("execute should succeed");

        assert_eq!(output, "<script>// \nconst x = 1;</script>");
    }

    #[test]
    fn script_line_comment_mode_with_crlf_is_terminated() {
        let template = Template::new("script")
            .parse("<script>// {{.A}}\r\nconst x = {{.B}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "inject", "B": 1}))
            .expect("execute should succeed");

        assert_eq!(output, "<script>// \r\nconst x = 1;</script>");
    }

    #[test]
    fn script_line_comment_mode_with_carriage_return_only_is_terminated() {
        let template = Template::new("script")
            .parse("<script>// {{.A}}\rconst x = {{.B}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "inject", "B": 1}))
            .expect("execute should succeed");

        assert_eq!(output, "<script>// \rconst x = 1;</script>");
    }

    #[test]
    fn script_line_comment_mode_with_unicode_line_separator_is_terminated() {
        let template = Template::new("script")
            .parse("<script>// {{.A}}\u{2028}const x = {{.B}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "inject", "B": 1}))
            .expect("execute should succeed");

        assert_eq!(output, "<script>// \u{2028}const x = 1;</script>");
    }

    #[test]
    fn script_line_comment_mode_with_unicode_paragraph_separator_is_terminated() {
        let template = Template::new("script")
            .parse("<script>// {{.A}}\u{2029}const x = {{.B}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "inject", "B": 1}))
            .expect("execute should succeed");

        assert_eq!(output, "<script>// \u{2029}const x = 1;</script>");
    }

    #[test]
    fn script_expr_mode_with_unicode_line_separator_is_preserved() {
        let template = Template::new("script")
            .parse("<script>const x = 1;\u{2028}const y = {{.Y}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Y": 2}))
            .expect("execute should succeed");

        assert_eq!(output, "<script>const x = 1;\u{2028}const y = 2;</script>");
    }

    #[test]
    fn script_expr_mode_with_unicode_paragraph_separator_is_preserved() {
        let template = Template::new("script")
            .parse("<script>const x = 1;\u{2029}const y = {{.Y}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Y": 2}))
            .expect("execute should succeed");

        assert_eq!(output, "<script>const x = 1;\u{2029}const y = 2;</script>");
    }

    #[test]
    fn script_line_comment_mode_ignores_template_inner_close_tag() {
        let template = Template::new("script")
            .parse("<script>// </script>\nconst x = {{.X}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"X": "<x>"}))
            .expect("execute to succeed");

        assert_eq!(
            output,
            "<script>// </script>\nconst x = \"\\u003cx\\u003e\";</script>"
        );
    }

    #[test]
    fn style_line_comment_mode_with_crlf_is_terminated() {
        let template = Template::new("style")
            .parse("<style>// {{.A}}\r\n.a { color: red; }</style>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "<x>"}))
            .expect("execute should succeed");

        assert_eq!(output, "<style>// \r\n.a { color: red; }</style>");
    }

    #[test]
    fn style_line_comment_mode_with_carriage_return_only_is_terminated() {
        let template = Template::new("style")
            .parse("<style>// {{.A}}\r.a { color: red; }</style>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "<x>"}))
            .expect("execute should succeed");

        assert_eq!(output, "<style>// \r.a { color: red; }</style>");
    }

    #[test]
    fn style_line_comment_mode_with_form_feed_is_terminated() {
        let template = Template::new("style")
            .parse("<style>// {{.A}}\x0c.a { color: red; }</style>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "<x>"}))
            .expect("execute should succeed");

        assert_eq!(output, "<style>// \x0c.a { color: red; }</style>");
    }

    #[test]
    fn style_line_comment_mode_with_unicode_line_separator_is_terminated() {
        let template = Template::new("style")
            .parse("<style>// {{.A}}\u{2028}.a { color: red; }</style>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "<x>"}))
            .expect("execute to succeed");

        assert_eq!(output, "<style>// \u{2028}.a { color: red; }</style>");
    }

    #[test]
    fn style_line_comment_mode_with_unicode_paragraph_separator_is_terminated() {
        let template = Template::new("style")
            .parse("<style>// {{.A}}\u{2029}.a { color: red; }</style>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "<x>"}))
            .expect("execute to succeed");

        assert_eq!(output, "<style>// \u{2029}.a { color: red; }</style>");
    }

    #[test]
    fn script_block_comment_mode_ignores_insertions() {
        let template = Template::new("script")
            .parse("<script>/* {{.A}} */const x = 1;</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "inject"}))
            .expect("execute should succeed");

        assert_eq!(output, "<script>/*  */const x = 1;</script>");
    }

    #[test]
    fn script_block_comment_mode_ignores_template_inner_close_tag() {
        let template = Template::new("script")
            .parse("<script>/* </script> */const x = {{.X}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"X": "<x>"}))
            .expect("execute to succeed");

        assert_eq!(
            output,
            "<script>/* </script> */const x = \"\\u003cx\\u003e\";</script>"
        );
    }

    #[test]
    fn script_html_like_comment_mode_ignores_insertions() {
        let template = Template::new("script")
            .parse("<script>before <!-- beep\nbetween\nbefore-->boop\n</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "<script>before \nbetween\nbefore\n</script>");
    }

    #[test]
    fn script_hashbang_comment_mode_ignores_insertions() {
        let template = Template::new("script")
            .parse("<script>#! beep\n</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "<script>\n</script>");
    }

    #[test]
    fn script_hashbang_comment_mode_ignores_template_call_insertions() {
        let template = Template::new("script")
            .parse(
                "{{define \"injected\"}}{{.}}{{end}}<script>#! {{template \"injected\" .A}}\n</script>",
            )
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "pwned()"}))
            .expect("execute to succeed");

        assert_eq!(output, "<script>\n</script>");
    }

    #[test]
    fn script_hashbang_comment_mode_ignores_template_inner_close_tag() {
        let template = Template::new("script")
            .parse("<script>#! </script>\nconst x = {{.X}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"X": "<x>"}))
            .expect("execute to succeed");

        assert_eq!(output, "<script>\nconst x = \"\\u003cx\\u003e\";</script>");
    }

    #[test]
    fn script_hashbang_comment_mode_with_crlf_is_terminated() {
        let template = Template::new("script")
            .parse("<script>#! {{.A}}\r\nconst x = 1;</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "<x>"}))
            .expect("execute to succeed");

        assert_eq!(output, "<script>\r\nconst x = 1;</script>");
    }

    #[test]
    fn script_hashbang_comment_mode_with_carriage_return_only_is_terminated() {
        let template = Template::new("script")
            .parse("<script>#! {{.A}}\rconst x = 1;</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "<x>"}))
            .expect("execute to succeed");

        assert_eq!(output, "<script>\rconst x = 1;</script>");
    }

    #[test]
    fn script_hashbang_comment_mode_with_unicode_line_separator_is_terminated() {
        let template = Template::new("script")
            .parse("<script>#! {{.A}}\u{2028}const x = 1;</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "<x>"}))
            .expect("execute to succeed");

        assert_eq!(output, "<script>\u{2028}const x = 1;</script>");
    }

    #[test]
    fn style_line_comment_mode_ignores_insertions() {
        let template = Template::new("style")
            .parse("<style>// {{.A}}\n.a { color: red; }</style>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "<x>"}))
            .expect("execute should succeed");

        assert_eq!(output, "<style>// \n.a { color: red; }</style>");
    }

    #[test]
    fn style_line_comment_mode_ignores_template_call_insertions() {
        let template = Template::new("style")
            .parse(
                "{{define \"injected\"}}{{.}}{{end}}<style>// {{template \"injected\" .A}}\n.a { color: red; }</style>",
            )
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "<x>"}))
            .expect("execute to succeed");

        assert_eq!(output, "<style>// \n.a { color: red; }</style>");
    }

    #[test]
    fn style_line_comment_mode_ignores_template_inner_close_tag() {
        let template = Template::new("style")
            .parse(
                "{{define \"injected\"}}{{.}}{{end}}<style>// </style>\n.a{color: {{template \"injected\" .A}}}</style>",
            )
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "</style>"}))
            .expect("execute to succeed");

        assert_eq!(
            output,
            "<style>// </style>\n.a{color: \\3C \\2F style\\3E }</style>"
        );
    }

    #[test]
    fn style_block_comment_mode_ignores_insertions() {
        let template = Template::new("style")
            .parse("<style>/* {{.A}} */.a { color: red; }</style>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "<x>"}))
            .expect("execute to succeed");

        assert_eq!(output, "<style>/*  */.a { color: red; }</style>");
    }

    #[test]
    fn style_block_comment_mode_ignores_template_call_insertions() {
        let template = Template::new("style")
            .parse(
                "{{define \"injected\"}}{{.}}{{end}}<style>/* {{template \"injected\" .A}} */.a { color: red; }</style>",
            )
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "<x>"}))
            .expect("execute to succeed");

        assert_eq!(output, "<style>/*  */.a { color: red; }</style>");
    }

    #[test]
    fn style_block_comment_mode_ignores_template_inner_close_tag() {
        let template = Template::new("style")
            .parse(
                "{{define \"injected\"}}{{.}}{{end}}<style>/* </style> */.a{color: {{template \"injected\" .A}}}</style>",
            )
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "</style>"}))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<style>/* </style> */.a{color: \\3C \\2F style\\3E }</style>"
        );
    }

    #[test]
    fn style_string_mode_ignores_close_tag_in_quotes() {
        let template = Template::new("style")
            .parse(
                "{{define \"injected\"}}{{.}}{{end}}<style>.a{content:\"</style>\";color: {{template \"injected\" .A}}}</style>",
            )
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "</style>"}))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<style>.a{content:\"</style>\";color: \\3C \\2F style\\3E }</style>"
        );
    }

    #[test]
    fn style_expr_context_escapes_template_call_values() {
        let template = Template::new("style")
            .parse(
                "{{define \"injected\"}}{{.}}{{end}}<style>.a{color: {{template \"injected\" .A}}}</style>",
            )
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "</script>"}))
            .expect("execute should succeed");

        assert_eq!(output, "<style>.a{color: \\3C \\2F script\\3E }</style>");
    }

    #[test]
    fn style_string_context_escapes_css_string_tokens() {
        let template = Template::new("style")
            .parse("<style>.x{content:'{{.S}}';}</style>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"S": "a'b\\c"}))
            .expect("execute should succeed");

        assert_eq!(output, "<style>.x{content:'a\\'b\\\\c';}</style>");
    }

    #[test]
    fn script_string_context_escapes_without_double_quoting() {
        let template = Template::new("script")
            .parse("<script>const s = \"{{.S}}\";</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"S": "\"</script><x>"}))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<script>const s = \"\\\"\\x3C/script\\x3E\\x3Cx\\x3E\";</script>"
        );
    }

    #[test]
    fn urlquery_function_matches_percent_encoding_behavior() {
        let template = Template::new("urlquery")
            .parse("{{urlquery .Query}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Query": "a=b c&x=<y>"}))
            .expect("execute should succeed");

        assert_eq!(output, "a%3Db%20c%26x%3D%3Cy%3E");
    }

    #[test]
    fn method_resolution_supports_niladic_method_lookup() {
        let template = Template::new("method")
            .add_method("FullName", |receiver: &Value, args: &[Value]| {
                if !args.is_empty() {
                    return Err(TemplateError::Render(
                        "FullName expects no arguments".to_string(),
                    ));
                }
                let first = lookup_path(receiver, &[String::from("First")]).to_plain_string();
                let last = lookup_path(receiver, &[String::from("Last")]).to_plain_string();
                Ok(Value::from(format!("{first} {last}")))
            })
            .parse("{{.User.FullName}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"User": {"First": "Ada", "Last": "Lovelace"}}))
            .expect("execute should succeed");

        assert_eq!(output, "Ada Lovelace");
    }

    #[test]
    fn method_resolution_supports_arguments() {
        let template = Template::new("method")
            .add_method("Greet", |receiver: &Value, args: &[Value]| {
                if args.len() != 1 {
                    return Err(TemplateError::Render(
                        "Greet expects one argument".to_string(),
                    ));
                }
                let name = lookup_path(receiver, &[String::from("Name")]).to_plain_string();
                Ok(Value::from(format!(
                    "{}:{}",
                    name,
                    args[0].to_plain_string()
                )))
            })
            .parse("{{.User.Greet \"world\"}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"User": {"Name": "hello"}}))
            .expect("execute should succeed");

        assert_eq!(output, "hello:world");
    }

    #[test]
    fn lookup_path_supports_intermediate_method_lookup() {
        let template = Template::new("method")
            .add_method("UpperNested", |receiver: &Value, args: &[Value]| {
                if !args.is_empty() {
                    return Err(TemplateError::Render(
                        "UpperNested expects no arguments".to_string(),
                    ));
                }
                let name = lookup_path(receiver, &[String::from("Name")]).to_plain_string();
                Ok(Value::from(json!({ "Name": name.to_uppercase() })))
            })
            .parse("{{.User.UpperNested.Name}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"User": {"Name": "alice"}}))
            .expect("execute should succeed");

        assert_eq!(output, "ALICE");
    }

    #[test]
    fn field_lookup_has_priority_over_method_name() {
        let template = Template::new("method")
            .add_method("Name", |_: &Value, _: &[Value]| Ok(Value::from("method")))
            .parse("{{.User.Name}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"User": {"Name": "field"}}))
            .expect("execute should succeed");

        assert_eq!(output, "field");
    }

    #[test]
    fn root_identifier_missing_key_uses_method_resolution() {
        let template = Template::new("method")
            .add_method("RootName", |_receiver: &Value, args: &[Value]| {
                if !args.is_empty() {
                    return Err(TemplateError::Render(
                        "RootName expects no arguments".to_string(),
                    ));
                }
                Ok(Value::from("resolved-by-method"))
            })
            .parse("{{RootName}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"X": "y"}))
            .expect("execute should succeed");

        assert_eq!(output, "resolved-by-method");
    }

    #[test]
    fn root_identifier_prefers_root_field_over_method() {
        let template = Template::new("method")
            .add_method("Name", |_: &Value, _args: &[Value]| {
                Ok(Value::from("from-method"))
            })
            .parse("{{Name}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Name": "from-root"}))
            .expect("execute should succeed");

        assert_eq!(output, "from-root");
    }

    #[test]
    fn identifier_missingkey_default_renders_no_value_marker() {
        let template = Template::new("missing")
            .parse("{{Name}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "&lt;no value&gt;");
    }

    #[test]
    fn identifier_missingkey_zero_renders_empty_string() {
        let template = Template::new("missing")
            .option("missingkey=zero")
            .expect("option should succeed")
            .parse("{{Name}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "");
    }

    #[test]
    fn identifier_missingkey_error_returns_execution_error() {
        let template = Template::new("missing")
            .option("missingkey=error")
            .expect("option should succeed")
            .parse("{{Name}}")
            .expect("parse should succeed");

        let error = template
            .execute_to_string(&json!({}))
            .expect_err("execute should fail");

        assert!(error.to_string().contains("map has no entry"));
        assert_eq!(error.code(), TemplateErrorCode::ErrMissingKey);
    }

    #[test]
    fn missingkey_default_renders_no_value_marker() {
        let template = Template::new("missing")
            .parse("{{.Missing}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "&lt;no value&gt;");
    }

    #[test]
    fn missingkey_zero_renders_empty_string() {
        let template = Template::new("missing")
            .option("missingkey=zero")
            .expect("option should succeed")
            .parse("{{.Missing}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "");
    }

    #[test]
    fn missingkey_error_returns_execution_error() {
        let template = Template::new("missing")
            .option("missingkey=error")
            .expect("option should succeed")
            .parse("{{.Missing}}")
            .expect("parse should succeed");

        let error = template
            .execute_to_string(&json!({}))
            .expect_err("execute should fail");

        assert!(error.to_string().contains("map has no entry"));
        assert_eq!(error.code(), TemplateErrorCode::ErrMissingKey);
    }

    #[test]
    fn printf_and_println_are_supported() {
        let template = Template::new("fmt")
            .parse("{{printf \"%s:%d\" .Name .Age}}|{{println .Name .Age}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Name": "alice", "Age": 20}))
            .expect("execute should succeed");

        assert_eq!(output, "alice:20|alice 20\n");
    }

    #[test]
    fn parenthesized_pipeline_can_be_used_as_function_argument() {
        let template = Template::new("paren-pipeline")
            .parse("{{printf \"%s\" (.Lang | printf \"%s!\")}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Lang": "ja"}))
            .expect("execute should succeed");

        assert_eq!(output, "ja!");
    }

    #[test]
    fn template_call_accepts_parenthesized_data_expression() {
        let template = Template::new("base")
            .add_func("dict", |args: &[Value]| {
                if args.len() != 2 {
                    return Err(TemplateError::Render("dict expects two args".to_string()));
                }
                let key = args[0].to_plain_string();
                let value = JsonValue::String(args[1].to_plain_string());
                let mut map = serde_json::Map::new();
                map.insert(key, value);
                Ok(Value::from(JsonValue::Object(map)))
            })
            .parse("{{define \"x\"}}{{.k}}{{end}}{{template \"x\" (dict \"k\" \"v\")}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "v");
    }

    #[test]
    fn parenthesized_expression_supports_postfix_field_access() {
        let template = Template::new("paren-field")
            .parse("{{(index .Page.X 0).Label}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({
                "Page": {
                    "X": [
                        {"Label": "first"}
                    ]
                }
            }))
            .expect("execute should succeed");

        assert_eq!(output, "first");
    }

    #[test]
    fn parse_rejects_unknown_function_calls_in_parenthesized_subexpr() {
        let error = match Template::new("main").parse("{{print (unknown 1)}}") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("function `unknown` is not registered")
        );
    }

    #[test]
    fn slice_function_supports_string_and_array() {
        let template = Template::new("slice")
            .parse("{{slice .S 1 4}}|{{slice .A 1 3}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"S": "abcdef", "A": ["x", "y", "z", "w"]}))
            .expect("execute should succeed");

        assert_eq!(output, "bcd|[&#34;y&#34;,&#34;z&#34;]");
    }

    #[test]
    fn call_function_invokes_registered_function_value() {
        let template = Template::new("call")
            .add_func("join2", |args: &[Value]| {
                if args.len() != 2 {
                    return Err(TemplateError::Render("join2 expects two args".to_string()));
                }
                Ok(Value::from(format!(
                    "{}-{}",
                    args[0].to_plain_string(),
                    args[1].to_plain_string()
                )))
            })
            .parse("{{call join2 \"a\" \"b\"}}|{{call \"join2\" \"c\" \"d\"}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "a-b|c-d");
    }

    #[test]
    fn js_function_outputs_sanitized_json() {
        let template = Template::new("js")
            .parse("{{js .X}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"X": "<tag>"}))
            .expect("execute should succeed");

        assert_eq!(output, "\"\\u003ctag\\u003e\"");
    }

    #[test]
    fn range_supports_assignment_with_equal() {
        let template = Template::new("range-eq")
            .parse("{{$i := 0}}{{$v := \"\"}}{{range $i, $v = .Items}}{{$i}}={{$v}};{{end}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Items": ["a", "b"]}))
            .expect("execute should succeed");

        assert_eq!(output, "0=a;1=b;");
    }

    #[test]
    fn with_supports_else_with_chain() {
        let template = Template::new("with")
            .parse("{{with .A}}A{{else with .B}}B{{else}}C{{end}}")
            .expect("parse should succeed");

        let out_a = template
            .execute_to_string(&json!({"A": "x", "B": "y"}))
            .expect("execute should succeed");
        let out_b = template
            .execute_to_string(&json!({"A": null, "B": "y"}))
            .expect("execute should succeed");
        let out_c = template
            .execute_to_string(&json!({"A": null, "B": null}))
            .expect("execute should succeed");

        assert_eq!(out_a, "A");
        assert_eq!(out_b, "B");
        assert_eq!(out_c, "C");
    }

    #[test]
    fn break_and_continue_work_inside_range() {
        let template = Template::new("loop")
            .parse(
                "{{range .Items}}{{if eq . \"skip\"}}{{continue}}{{end}}{{if eq . \"stop\"}}{{break}}{{end}}{{.}};{{end}}",
            )
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Items": ["a", "skip", "b", "stop", "c"]}))
            .expect("execute should succeed");

        assert_eq!(output, "a;b;");
    }

    #[test]
    fn range_else_continue_is_treated_as_no_op() {
        let template = Template::new("range-else-continue")
            .parse("{{range .Items}}{{else}}{{continue}}{{end}}ok")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Items": []}))
            .expect("execute should succeed");

        assert_eq!(output, "ok");
    }

    #[test]
    fn break_outside_range_returns_error() {
        let error = match Template::new("break").parse("{{break}}") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("break action is not inside range")
        );
    }

    #[test]
    fn delims_allows_custom_action_delimiters() {
        let template = Template::new("delims")
            .delims("[[", "]]")
            .parse("<p>[[.Name]]</p>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Name": "alice"}))
            .expect("execute should succeed");

        assert_eq!(output, "<p>alice</p>");
    }

    #[test]
    fn delims_empty_values_fall_back_to_go_defaults() {
        let delim_pairs = [("", ""), ("{{", "}}"), ("[[", "]]"), ("(日)", "(本)")];

        for (left, right) in delim_pairs {
            let true_left = if left.is_empty() { "{{" } else { left };
            let true_right = if right.is_empty() { "}}" } else { right };
            let source = format!(
                "{true_left}.Name{true_right}{true_left}/*x*/{true_right}{true_left}\"{true_left}\"{true_right}"
            );

            let template = Template::new("delims")
                .delims(left, right)
                .parse(&source)
                .expect("parse should succeed");

            let output = template
                .execute_to_string(&json!({"Name": "Hello"}))
                .expect("execute should succeed");

            assert_eq!(output, format!("Hello{true_left}"));
        }
    }

    #[test]
    fn empty_define_does_not_override_existing_template_body() {
        let template = Template::new("root")
            .parse("{{define \"x\"}}foo{{end}}{{template \"x\" .}}")
            .expect("parse should succeed")
            .parse("{{define \"x\"}}   {{/* comment */}} \n\t {{end}}")
            .expect("parse should succeed");

        let root_output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        let named_output = template
            .execute_template_to_string("x", &json!({}))
            .expect("execute should succeed");

        assert_eq!(root_output, "foo");
        assert_eq!(named_output, "foo");
    }

    #[test]
    fn funcs_and_methods_accept_go_compatible_names() {
        let names = ["_", "a", "a1", "Ӵ"];

        for name in names {
            let mut funcs = FuncMap::new();
            funcs.insert(name.to_string(), Arc::new(test_func) as Function);
            let _ = Template::new("funcs").funcs(funcs);

            let mut methods = MethodMap::new();
            methods.insert(name.to_string(), Arc::new(test_method) as Method);
            let _ = Template::new("methods").methods(methods);
        }
    }

    #[test]
    fn funcs_and_methods_reject_invalid_names() {
        let bad_names = ["", "2", "a-b"];

        for name in bad_names {
            let mut funcs = FuncMap::new();
            funcs.insert(name.to_string(), Arc::new(test_func) as Function);
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _ = Template::new("funcs").funcs(funcs);
                }))
                .is_err(),
                "func name `{name}` should panic"
            );

            let mut methods = MethodMap::new();
            methods.insert(name.to_string(), Arc::new(test_method) as Method);
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _ = Template::new("methods").methods(methods);
                }))
                .is_err(),
                "method name `{name}` should panic"
            );
        }
    }

    #[test]
    fn add_func_and_add_method_reject_invalid_names() {
        let func_registration = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ =
                Template::new("funcs").add_func("2bad", |_args: &[Value]| Ok(Value::from("ok")));
        }));
        assert!(
            func_registration.is_err(),
            "invalid add_func name should panic"
        );

        let method_registration = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = Template::new("methods")
                .add_method("bad-name", |_receiver: &Value, _args: &[Value]| {
                    Ok(Value::from("ok"))
                });
        }));
        assert!(
            method_registration.is_err(),
            "invalid add_method name should panic"
        );
    }

    #[test]
    fn parse_rejects_unknown_function_calls() {
        let error = match Template::new("main").parse("{{unknown 1}}") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("function `unknown` is not registered")
        );
    }

    #[test]
    fn parse_accepts_registered_function_calls() {
        let template = Template::new("main")
            .add_func("myfunc", |_args: &[Value]| Ok(Value::from("ok")))
            .parse("{{myfunc 1}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "ok");
    }

    #[test]
    fn predefined_escaper_html_is_rejected_when_not_last_in_pipeline() {
        let error = match Template::new("escaper").parse("{{.X | html | print}}") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("predefined escaper \"html\" disallowed in template")
        );
        assert_eq!(error.code(), TemplateErrorCode::ErrPredefinedEscaper);
    }

    #[test]
    fn predefined_escaper_urlquery_is_rejected_when_not_last_in_pipeline() {
        let error = match Template::new("escaper").parse("{{.X | urlquery | print}}") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("predefined escaper \"urlquery\" disallowed in template")
        );
        assert_eq!(error.code(), TemplateErrorCode::ErrPredefinedEscaper);
    }

    #[test]
    fn predefined_escaper_is_allowed_at_pipeline_tail() {
        let template = Template::new("escaper")
            .parse("{{.X | html}}|{{.Y | urlquery}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"X": "<tag>", "Y": "a b"}))
            .expect("execute should succeed");

        assert_eq!(output, "&lt;tag&gt;|a%20b");
    }

    #[test]
    fn clone_template_preserves_behavior() {
        let template = Template::new("clone")
            .add_func("upper", |args: &[Value]| {
                let value = args
                    .first()
                    .ok_or_else(|| TemplateError::Render("upper expects one arg".to_string()))?;
                Ok(Value::from(value.to_plain_string().to_uppercase()))
            })
            .parse("{{.Name | upper}}")
            .expect("parse should succeed");

        let cloned = template.clone_template().expect("clone should succeed");
        let output = cloned
            .execute_to_string(&json!({"Name": "alice"}))
            .expect("execute should succeed");

        assert_eq!(output, "ALICE");
    }

    #[test]
    fn clone_template_fails_after_execution() {
        let template = Template::new("clone")
            .parse("{{.Name}}")
            .expect("parse should succeed");

        let rendered = template
            .execute_to_string(&json!({"Name": "alice"}))
            .expect("execute should succeed");
        assert_eq!(rendered, "alice");

        let error = match template.clone_template() {
            Ok(_) => panic!("clone should fail after execute"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cannot be parsed or cloned"));
    }

    #[test]
    fn clone_creates_isolated_template_namespace() {
        let base = Template::new("base")
            .parse("{{define \"shared\"}}shared{{end}}")
            .expect("parse should succeed");

        let cloned = base
            .Clone()
            .expect("clone should succeed")
            .parse("{{define \"child\"}}child only{{end}}")
            .expect("parse on clone should succeed");

        assert!(base.lookup("child").is_none());
        let cloned_output = cloned
            .execute_template_to_string("child", &json!({}))
            .expect("cloned execute should succeed");

        assert_eq!(cloned_output, "child only");
    }

    #[test]
    fn clone_api_is_independent_from_clone_template() {
        let template = Template::new("base")
            .parse("{{.Name}}")
            .expect("parse should succeed");
        let cloned = template.Clone().expect("Clone should succeed");

        let output = cloned
            .execute_to_string(&json!({"Name": "value"}))
            .expect("execute should succeed");
        assert_eq!(output, "value");
    }

    #[test]
    fn clone_preserves_option_and_delims() {
        let template = Template::new("base")
            .delims("[[", "]]")
            .option("missingkey=zero")
            .expect("option should succeed")
            .parse("[[.Missing]]")
            .expect("parse should succeed");

        let cloned = template.Clone().expect("Clone should succeed");
        let output = cloned
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        assert_eq!(output, "");
    }

    #[test]
    fn clone_carries_functions_and_methods() {
        let template = Template::new("clone-func")
            .add_func("upper", |args: &[Value]| {
                let value = args
                    .first()
                    .ok_or_else(|| TemplateError::Render("upper expects one arg".to_string()))?;
                Ok(Value::from(value.to_plain_string().to_uppercase()))
            })
            .add_method("Join", |_receiver: &Value, args: &[Value]| {
                if args.len() != 2 {
                    return Err(TemplateError::Render("Join expects two args".to_string()));
                }
                Ok(Value::from(format!(
                    "{}:{}",
                    args[0].to_plain_string(),
                    args[1].to_plain_string()
                )))
            })
            .parse("{{.Name | upper}}|{{.Obj.Join \"a\" \"b\"}}")
            .expect("parse should succeed");

        let cloned = template.Clone().expect("Clone should succeed");
        let output = cloned
            .execute_to_string(&json!({"Name": "rust", "Obj": {"Name": "x"}}))
            .expect("execute should succeed");
        assert_eq!(output, "RUST|a:b");
    }

    #[test]
    fn clone_can_execute_parsed_text_independently() {
        let template = Template::new("clone-base")
            .parse("orig {{define \"foo\"}}foo{{end}}")
            .expect("parse should succeed");

        let cloned = template
            .Clone()
            .expect("clone should succeed")
            .parse("extra")
            .expect("parse on clone should succeed");

        assert_eq!(cloned.Templates().len(), 2);
        assert_eq!(
            cloned
                .execute_to_string(&json!({}))
                .expect("execute should succeed"),
            "extra"
        );
        assert_eq!(
            template
                .execute_to_string(&json!({}))
                .expect("execute should succeed"),
            "orig "
        );
    }

    #[test]
    fn clone_fails_after_execution() {
        let template = Template::new("clone-fail")
            .parse("{{.Name}}")
            .expect("parse should succeed");

        let _ = template
            .execute_to_string(&json!({"Name": "value"}))
            .expect("execute should succeed");

        let error = match template.Clone() {
            Ok(_) => panic!("Clone should fail after execute"),
            Err(err) => err,
        };
        assert!(error.to_string().contains("cannot be parsed or cloned"));
    }

    #[test]
    fn templates_includes_all_associated_templates() {
        let template = Template::new("base")
            .parse("base {{define \"footer\"}}footer{{end}}{{define \"header\"}}header{{end}}")
            .expect("parse should succeed");

        let names = template
            .Templates()
            .into_iter()
            .map(|tpl| tpl.name().to_string())
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"base".to_string()));
        assert!(names.contains(&"header".to_string()));
        assert!(names.contains(&"footer".to_string()));
    }

    #[test]
    fn defined_templates_matches_prefixed_sorted_list() {
        let template = Template::new("base")
            .parse("base {{define \"footer\"}}footer{{end}}{{define \"header\"}}header{{end}}")
            .expect("parse should succeed");

        assert_eq!(
            template.DefinedTemplates(),
            "; defined templates are: base, footer, header"
        );
    }

    #[test]
    fn templates_are_sorted_and_stable() {
        let template = Template::new("base")
            .parse("{{define \"z\"}}z{{end}}{{define \"a\"}}a{{end}}")
            .expect("parse should succeed");

        let names = template
            .Templates()
            .into_iter()
            .map(|tpl| tpl.name().to_string())
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["a", "base", "z"]);
    }

    #[test]
    fn namespace_state_is_shared_between_templates_and_templates_api() {
        let template = Template::new("base")
            .parse("{{define \"shared\"}}shared{{end}}")
            .expect("parse should succeed");

        let extender = template
            .Templates()
            .into_iter()
            .next()
            .expect("templates should include at least one entry");
        let extender = extender
            .parse("{{define \"via_namespace\"}}via namespace{{end}}")
            .expect("parse should succeed");

        assert!(template.has_template("via_namespace"));
        assert_eq!(
            template
                .execute_template_to_string("via_namespace", &json!({}))
                .expect("template execute should succeed"),
            "via namespace"
        );
        assert_eq!(extender.Templates().len(), template.Templates().len());
    }

    #[test]
    fn parse_fails_after_execution() {
        let template = Template::new("parse-after-exec")
            .parse("{{.Name}}")
            .expect("parse should succeed");

        let rendered = template
            .execute_to_string(&json!({"Name": "alice"}))
            .expect("execute should succeed");
        assert_eq!(rendered, "alice");

        let error = match template.parse("{{.Other}}") {
            Ok(_) => panic!("parse should fail after execute"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cannot be parsed or cloned"));
    }

    #[test]
    fn redefine_non_empty_after_execution_is_rejected() {
        let template = Template::new("redefine")
            .parse("foo")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        assert_eq!(output, "foo");

        let error = match template.parse("bar") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cannot be parsed or cloned"));
    }

    #[test]
    fn redefine_named_template_after_execution_is_rejected() {
        let template = Template::new("redefine")
            .parse("{{define \"x\"}}foo{{end}}")
            .expect("parse should succeed");

        let output = template
            .execute_template_to_string("x", &json!({}))
            .expect("execute should succeed");
        assert_eq!(output, "foo");

        let error = match template.parse("{{define \"x\"}}bar{{end}}") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cannot be parsed or cloned"));
    }

    #[test]
    fn redefine_empty_after_execution_is_rejected_and_preserves_output() {
        let template = Template::new("redefine-empty")
            .parse("")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        assert_eq!(output, "");

        let error = match template.clone().parse("foo") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cannot be parsed or cloned"));

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should continue to succeed");
        assert_eq!(output, "");
    }

    #[test]
    fn redefine_lookup_template_after_execution_is_rejected() {
        let root = Template::new("redefine")
            .parse("{{define \"x\"}}foo{{end}}")
            .expect("parse should succeed");
        let named = root.lookup("x").expect("template x should exist");

        let output = named
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        assert_eq!(output, "foo");

        let error = match named.parse("bar") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cannot be parsed or cloned"));

        let output = root
            .execute_template_to_string("x", &json!({}))
            .expect("execute should still succeed");
        assert_eq!(output, "foo");
    }

    #[test]
    fn redefine_before_execution_updates_named_template_body() {
        let template = Template::new("redefine")
            .parse("{{define \"x\"}}foo{{end}}{{template \"x\" .}}")
            .expect("parse should succeed")
            .parse("{{define \"x\"}}bar{{end}}{{template \"x\" .}}")
            .expect("parse should succeed");

        let root = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        let named = template
            .execute_template_to_string("x", &json!({}))
            .expect("execute should succeed");

        assert_eq!(root, "bar");
        assert_eq!(named, "bar");
    }

    #[test]
    fn redefine_after_non_execution_is_rejected_and_keeps_previous_definition() {
        let template = Template::new("redefine")
            .parse("{{if .}}<{{template \"x\" .}}>{{end}}{{define \"x\"}}foo{{end}}")
            .expect("parse should succeed");

        let output_false = template
            .execute_to_string(&json!(0))
            .expect("execute should succeed");
        assert_eq!(output_false, "");

        let error = match template.clone().parse("{{define \"x\"}}bar{{end}}") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cannot be parsed or cloned"));

        let output_true = template
            .execute_to_string(&json!(1))
            .expect("execute should succeed");
        assert_eq!(output_true, "<foo>");
    }

    #[test]
    fn redefine_after_named_execution_is_rejected_and_keeps_previous_definition() {
        let template = Template::new("redefine")
            .parse("<{{template \"x\" .}}>{{define \"x\"}}foo{{end}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        assert_eq!(output, "<foo>");

        let error = match template.clone().parse("{{define \"x\"}}bar{{end}}") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cannot be parsed or cloned"));

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should still succeed");
        assert_eq!(output, "<foo>");
    }

    #[test]
    fn redefine_safety_prevents_post_execute_injection() {
        let template = Template::new("redefine")
            .parse("<html><a href=\"{{template \"x\" .}}\">{{define \"x\"}}{{end}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        assert_eq!(output, "<html><a href=\"\">");

        let error = match template
            .clone()
            .parse("{{define \"x\"}}\" bar=\"baz{{end}}")
        {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cannot be parsed or cloned"));

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should still succeed");
        assert_eq!(output, "<html><a href=\"\">");
    }

    #[test]
    fn redefine_top_use_prevents_post_execute_script_injection() {
        let template = Template::new("redefine")
            .parse("{{template \"x\" .}}{{.}}{{define \"x\"}}{{end}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!(42))
            .expect("execute should succeed");
        assert_eq!(output, "42");

        let error = match template.clone().parse("{{define \"x\"}}<script>{{end}}") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cannot be parsed or cloned"));

        let output = template
            .execute_to_string(&json!(42))
            .expect("execute should still succeed");
        assert_eq!(output, "42");
    }

    #[test]
    fn parser_apis_fail_after_execution() {
        let template = Template::new("redefine")
            .parse("")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        assert_eq!(output, "");

        let parse_files_error = match template.clone().parse_files(Vec::<&str>::new()) {
            Ok(_) => panic!("parse_files should fail"),
            Err(error) => error,
        };
        assert!(
            parse_files_error
                .to_string()
                .contains("cannot be parsed or cloned")
        );

        let parse_glob_error = match template.clone().parse_glob("*.no.template") {
            Ok(_) => panic!("parse_glob should fail"),
            Err(error) => error,
        };
        assert!(
            parse_glob_error
                .to_string()
                .contains("cannot be parsed or cloned")
        );

        let tree = Template::new("tree")
            .parse_tree("{{define \"t1\"}}x{{end}}")
            .expect("parse_tree should succeed");
        let add_tree_error = match template.clone().AddParseTree("t1", tree) {
            Ok(_) => panic!("AddParseTree should fail"),
            Err(error) => error,
        };
        assert!(
            add_tree_error
                .to_string()
                .contains("cannot be parsed or cloned")
        );
    }

    #[test]
    fn execute_to_string_is_safe_for_parallel_runs() {
        let template = Arc::new(
            Template::new("parallel")
                .parse("{{.Name}}:{{.Index}}")
                .expect("parse should succeed"),
        );

        let mut handles = Vec::new();
        for i in 0..8 {
            let template = Arc::clone(&template);
            handles.push(std::thread::spawn(move || {
                for n in 0..50 {
                    let output = template
                        .execute_to_string(&json!({"Name": "worker", "Index": i * 100 + n}))
                        .expect("execute should succeed");
                    assert_eq!(output, format!("worker:{}", i * 100 + n));
                }
            }));
        }

        for handle in handles {
            handle.join().expect("thread should succeed");
        }
    }

    #[test]
    fn clone_and_lookup_can_execute_in_parallel() {
        let base = Template::new("base")
            .parse("{{define \"item\"}}{{.Name}}-{{.Index}}{{end}}")
            .expect("parse should succeed");
        let cloned = Arc::new(base.Clone().expect("Clone should succeed"));
        let item = Arc::new(base.lookup("item").expect("item template should exist"));

        let mut handles = Vec::new();
        for i in 0..8 {
            let cloned = Arc::clone(&cloned);
            let item = Arc::clone(&item);
            handles.push(std::thread::spawn(move || {
                let left = cloned
                    .execute_template_to_string("item", &json!({"Name": "L", "Index": i}))
                    .expect("cloned execute should succeed");
                let right = item
                    .execute_to_string(&json!({"Name": "R", "Index": i}))
                    .expect("lookup execute should succeed");
                assert_eq!(left, format!("L-{i}"));
                assert_eq!(right, format!("R-{i}"));
            }));
        }

        for handle in handles {
            handle.join().expect("thread should succeed");
        }
    }

    #[test]
    fn range_over_map_is_sorted_by_key() {
        let template = Template::new("map")
            .parse("{{range $k, $v := .Map}}{{$k}}={{$v}};{{end}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Map": {"b": 2, "a": 1}}))
            .expect("execute should succeed");

        assert_eq!(output, "a=1;b=2;");
    }

    #[test]
    fn safe_types_can_bypass_matching_context_escaping() {
        let template = Template::new("safe-types")
            .add_func("to_url", |args: &[Value]| {
                let input = args.first().map(Value::to_plain_string).unwrap_or_default();
                Ok(Value::safe_url(input))
            })
            .add_func("to_js", |args: &[Value]| {
                let input = args.first().map(Value::to_plain_string).unwrap_or_default();
                Ok(Value::safe_js(input))
            })
            .add_func("to_css", |args: &[Value]| {
                let input = args.first().map(Value::to_plain_string).unwrap_or_default();
                Ok(Value::safe_css(input))
            })
            .parse(
                "<a href=\"{{to_url .U}}\">go</a><script>const s=\"{{to_js .S}}\";</script><style>.x{content:\"{{to_css .C}}\";}</style>",
            )
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({
                "U": "javascript:alert(1)",
                "S": "\\\"</script>",
                "C": "\\\"</style>"
            }))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<a href=\"javascript:alert(1)\">go</a><script>const s=\"\\\"</script>\";</script><style>.x{content:\"\\\"</style>\";}</style>"
        );
    }

    #[test]
    fn safe_html_attr_preserves_raw_markup_in_attribute_quotes() {
        let template = Template::new("attr-safe")
            .add_func("safe_html_attr_value", |args: &[Value]| {
                let value = args.first().map(Value::to_plain_string).unwrap_or_default();
                Ok(Value::safe_html_attr(value))
            })
            .parse("<a title=\"{{safe_html_attr_value .A}}\"></a>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "O'Reilly & <x>"}))
            .expect("execute should succeed");

        assert_eq!(output, "<a title=\"O'Reilly & <x>\"></a>");
    }

    #[test]
    fn method_invoke_can_receive_pipeline_value_as_last_argument() {
        let template = Template::new("pipeline-method")
            .add_method("Wrap", |_receiver: &Value, args: &[Value]| {
                if args.len() != 2 {
                    return Err(TemplateError::Render(
                        "Wrap expects two arguments".to_string(),
                    ));
                }
                Ok(Value::from(format!(
                    "{}{}",
                    args[0].to_plain_string(),
                    args[1].to_plain_string()
                )))
            })
            .parse("{{.Val | .Wrap \"pre-\"}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Val": "x"}))
            .expect("execute should succeed");

        assert_eq!(output, "pre-x");
    }

    #[test]
    fn eq_matches_if_any_argument_equals_first() {
        let template = Template::new("eq")
            .parse("{{eq .A .B .C}}/{{eq .A .B .D}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": 2, "B": 1, "C": 2, "D": 3}))
            .expect("execute should succeed");

        assert_eq!(output, "true/false");
    }

    #[test]
    fn parse_time_rejects_branch_context_mismatch() {
        let error = match Template::new("ambig")
            .parse("{{if .C}}<a href=\"{{else}}<a title=\"{{end}}{{.X}}\">")
        {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("branches end in different contexts")
        );
        assert_eq!(error.code(), TemplateErrorCode::ErrBranchEnd);
    }

    #[test]
    fn parse_time_allows_recursive_template_calls() {
        let template = Template::new("main")
            .parse("{{template \"main\" .}}")
            .expect("parse should succeed");

        let error = template
            .execute_to_string(&json!({}))
            .expect_err("execution should fail");
        assert!(error.to_string().contains("maximum execution depth"));
        assert_eq!(error.code(), TemplateErrorCode::ErrRender);
    }

    #[test]
    fn parse_time_allows_mutual_recursive_template_calls() {
        let template = Template::new("a")
            .parse(
            "{{define \"a\"}}{{template \"b\" .}}{{end}}{{define \"b\"}}{{template \"a\" .}}{{end}}{{template \"a\" .}}",
        )
            .expect("parse should succeed");

        let error = template
            .execute_template_to_string("a", &json!({}))
            .expect_err("execution should fail");
        assert!(error.to_string().contains("maximum execution depth"));
        assert_eq!(error.code(), TemplateErrorCode::ErrRender);
    }

    #[test]
    fn recursive_template_with_range_else_can_render() {
        let template = Template::new("main")
            .parse("{{range .Children}}{{template \"main\" .}}{{else}}{{.X}} {{end}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({
                "Children": [
                    {"X": "foo"},
                    {"X": "<bar>"},
                    {"Children": [{"X": "baz"}]}
                ]
            }))
            .expect("execute should succeed");

        assert_eq!(output, "foo &lt;bar&gt; baz ");
    }

    #[test]
    fn execute_fails_on_excessive_template_call_depth() {
        let chain_len = MAX_TEMPLATE_EXECUTION_DEPTH + 1;
        let mut source = String::from("{{template \"depth-1\" .}}");
        for depth in 1..=chain_len {
            source.push_str(&format!("{{{{define \"depth-{depth}\"}}}}"));
            if depth == chain_len {
                source.push_str("done");
            } else {
                source.push_str(&format!("{{{{template \"depth-{}\" .}}}}", depth + 1));
            }
            source.push_str("{{end}}");
        }

        let template = Template::new("main")
            .parse(&source)
            .expect("parse should succeed");

        let error = template
            .execute_template_to_string("main", &json!({}))
            .expect_err("execution should fail");

        assert!(error.to_string().contains("maximum execution depth"));
        assert_eq!(error.code(), TemplateErrorCode::ErrRender);
    }

    #[test]
    fn execute_missing_template_returns_not_defined_error_code() {
        let template = Template::new("main")
            .parse("{{.Name}}")
            .expect("parse should succeed");
        let error = match template.execute_template_to_string("missing", &json!({"Name": "alice"}))
        {
            Ok(_) => panic!("execution should fail"),
            Err(err) => err,
        };
        assert_eq!(error.code(), TemplateErrorCode::ErrNotDefined);
        assert!(error.to_string().contains("is not defined"));
    }

    #[test]
    fn parse_time_rejects_missing_template_reference() {
        let error = match Template::new("main").parse("<div>{{template \"missing\" .}}</div>") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("no such template"));
        assert_eq!(error.code(), TemplateErrorCode::ErrNoSuchTemplate);
    }

    #[test]
    fn template_error_info_exposes_name_and_reason() {
        let error = match Template::new("main").parse("<div>{{template \"missing\" .}}</div>") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };
        let info = error.info();
        assert_eq!(info.name, Some("missing".to_string()));
        assert!(info.reason.contains("no such template"));
        assert!(info.reason.contains("`missing`"));
        assert_eq!(info.line, None);
    }

    #[test]
    fn template_error_line_parser_supports_simple_errors() {
        let err = TemplateError::Parse("line 12: unclosed action (missing }})".to_string());
        let info = err.info();
        assert_eq!(info.line, Some(12));
        assert_eq!(err.line(), Some(12));
        assert_eq!(err.name(), None);
        assert_eq!(
            err.reason(),
            "line 12: unclosed action (missing }})".to_string()
        );
    }

    #[test]
    fn parse_error_code_maps_extended_categories() {
        assert_eq!(
            TemplateError::Parse("template `x` ends in a non-text context".to_string()).code(),
            TemplateErrorCode::ErrEndContext
        );
        assert_eq!(
            TemplateError::Parse("on range loop re-entry: context mismatch".to_string()).code(),
            TemplateErrorCode::ErrRangeLoopReentry
        );
        assert_eq!(
            TemplateError::Parse("unfinished JS regexp charset: \"[\"".to_string()).code(),
            TemplateErrorCode::ErrPartialCharset
        );
        assert_eq!(
            TemplateError::Parse("unfinished escape sequence in JS string: \"\\\\\"".to_string())
                .code(),
            TemplateErrorCode::ErrPartialEscape
        );
        assert_eq!(
            TemplateError::Parse("'/' could start a division or regexp".to_string()).code(),
            TemplateErrorCode::ErrSlashAmbig
        );
        assert_eq!(
            TemplateError::Parse("predefined escaper \"html\" disallowed in template".to_string())
                .code(),
            TemplateErrorCode::ErrPredefinedEscaper
        );
    }

    #[test]
    fn parse_time_rejects_action_after_js_escape_prefix() {
        let error =
            match Template::new("js-partial").parse(r#"<script>const s = "\{{.X}}";</script>"#) {
                Ok(_) => panic!("parse should fail"),
                Err(error) => error,
            };

        assert!(
            error
                .to_string()
                .contains("unfinished escape sequence in JS string")
        );
        assert_eq!(error.code(), TemplateErrorCode::ErrPartialEscape);
    }

    #[test]
    fn parse_time_rejects_action_after_css_escape_prefix() {
        let error =
            match Template::new("css-partial").parse(r#"<style>.x{content:"\{{.X}}";}</style>"#) {
                Ok(_) => panic!("parse should fail"),
                Err(error) => error,
            };

        assert!(
            error
                .to_string()
                .contains("unfinished escape sequence in CSS string")
        );
        assert_eq!(error.code(), TemplateErrorCode::ErrPartialEscape);
    }

    #[test]
    fn parse_time_allows_backtick_template_literal_with_regex_like_text() {
        let template = Template::new("backtick-regex-like")
            .parse(r#"<script>const s = `/[{{.X}}`;</script>"#)
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"X": "abc"}))
            .expect("execute should succeed");

        assert_eq!(output, r#"<script>const s = `/[abc`;</script>"#);
    }

    #[test]
    fn parse_time_rejects_action_inside_regexp_char_class() {
        let error = match Template::new("regexp-partial")
            .parse(r#"<script>const r = /foo[{{.Chars}}]/;</script>"#)
        {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("unfinished JS regexp charset"));
        assert_eq!(error.code(), TemplateErrorCode::ErrPartialCharset);
    }

    #[test]
    fn parse_time_rejects_slash_ambiguity_after_branch() {
        let error = match Template::new("slash-ambig")
            .parse(r#"<script>{{if .C}}var x = 1{{end}}/-{{.N}}/i.test(x)</script>"#)
        {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("could start a division or regexp")
        );
        assert_eq!(error.code(), TemplateErrorCode::ErrSlashAmbig);
    }

    #[test]
    fn parse_numbers_supports_go_style_numeric_literals() {
        let template = Template::new("numbers")
            .parse("{{1_2}}|{{1_2.3_4}}|{{0x0_1.e_0p+02}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "12|12.34|7.5");
    }

    #[test]
    fn parse_numbers_supports_go_style_numeric_bases_and_octal_zero_prefix() {
        let template = Template::new("numbers-base")
            .parse(
                "{{print 1234}}|{{print 12_34}}|{{print 0b101}}|{{print 0b_1_0_1}}|{{print 0B101}}|{{print 0o377}}|{{print 0o_3_7_7}}|{{print 0O377}}|{{print 0377}}|{{print 0x123}}|{{print 0x1_23}}|{{print 0X123ABC}}|{{print 123.4}}|{{print 0_0_1_2_3.4}}|{{print +0x1.ep+2}}|{{print +0x_1.e_0p+0_2}}|{{print +0X1.EP+2}}|{{print 1_2_3_4 7.5_00_00_00}}|{{print 1234 0x0_1.e_0p+02}}",
            )
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "1234|1234|5|5|5|255|255|255|255|291|291|1194684|123.4|123.4|7.5|7.5|7.5|12347.5|12347.5"
        );
    }

    #[test]
    fn parse_numbers_supports_leading_dot_and_sign_prefixed_formats() {
        let template = Template::new("numbers-dot")
            .parse("{{.5}}|{{+.5}}|{{-.5}}|{{.5e2}}|{{-0.5e-1}}|{{+0x1p+2}}|{{-0x1p-2}}|{{.0e1}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "0.5|0.5|-0.5|50|-0.05|4|-0.25|0");
    }

    #[test]
    fn parse_numbers_supports_signed_prefixed_integers() {
        let template = Template::new("numbers-signed-prefix")
            .parse("{{print -0b101}}|{{print +0B101}}|{{print -0o377}}|{{print +0377}}|{{print -0x1f}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "-5|5|-255|255|-31");
    }

    #[test]
    fn parse_numbers_reject_complex_like_literals() {
        let error = match Template::new("complex-literal").parse("{{1.5i}}") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };

        assert!(
            error.to_string().contains("unsupported token")
                || error.to_string().contains("illegal number syntax")
                || error.to_string().contains("bad number syntax"),
            "unexpected error message: {error}"
        );
    }

    #[test]
    fn parse_numbers_reject_invalid_underscore_forms() {
        let tests = [
            "{{1_}}",
            "{{1__2}}",
            "{{0x1_}}",
            "{{0x1p+_2}}",
            "{{0x1p+2_}}",
            "{{0b1_.}}",
            "{{+_1}}",
            "{{.0a}}",
            "{{.5_}}",
            "{{09}}",
            "{{-09}}",
            "{{+09}}",
            "{{0_9}}",
        ];

        for source in tests {
            let error = match Template::new("invalid-numeric").parse(source) {
                Ok(_) => panic!("parse should fail: {source}"),
                Err(error) => error,
            };

            assert!(
                error.to_string().contains("unsupported token")
                    || error.to_string().contains("bad number syntax")
                    || error.to_string().contains("illegal number syntax")
                    || error.to_string().contains("is not defined"),
                "unexpected error message: {error}"
            );
        }
    }

    #[test]
    fn parse_numbers_supports_scientific_notation_without_hex_prefix() {
        let template = Template::new("numbers-exp")
            .parse("{{print 1e1}}|{{print 1e+2}}|{{print 1e-2}}|{{print +1e3}}|{{print -1e3}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "10|100|0.01|1000|-1000");
    }

    #[test]
    fn parse_time_rejects_non_text_end_context() {
        let error = match Template::new("end").parse("<div title=\"{{.X}}") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("non-text context"));
        assert_eq!(error.code(), TemplateErrorCode::ErrEndContext);
    }

    #[test]
    fn parse_time_context_is_not_affected_by_prior_runtime_output() {
        let template = Template::new("stable")
            .parse("{{safe_html .Prefix}}<a href=\"{{.URL}}\">go</a>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({
                "Prefix": "<script>/* attacker controlled */",
                "URL": "javascript:alert(1)"
            }))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<script>/* attacker controlled */<a href=\"#ZgotmplZ\">go</a>"
        );
    }
}
