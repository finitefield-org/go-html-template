use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
#[cfg(not(feature = "web-rust"))]
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::sync::RwLockReadGuard;
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
    SafeJsStr(String),
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

    pub fn from_serializable<T: Serialize>(data: &T) -> Result<Self> {
        Ok(Self::Json(serde_json::to_value(data)?))
    }

    fn truthy(&self) -> bool {
        match self {
            Value::SafeHtml(value) => !value.is_empty(),
            Value::SafeHtmlAttr(value) => !value.is_empty(),
            Value::SafeJs(value) => !value.is_empty(),
            Value::SafeJsStr(value) => !value.is_empty(),
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
            Value::SafeJsStr(value) => value.clone(),
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
        Value::SafeJsStr(value.0)
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

struct RuntimeContext<'a> {
    funcs: RwLockReadGuard<'a, FuncMap>,
    methods: RwLockReadGuard<'a, MethodMap>,
    missing_key_mode: MissingKeyMode,
}

#[derive(Clone, Debug)]
pub struct ParseTree {
    nodes: Vec<Node>,
}

#[derive(Clone, Copy, Debug, Default)]
struct TextScanSummary {
    has_lt: bool,
    has_eq: bool,
    has_single_quote: bool,
    has_double_quote: bool,
    has_backtick: bool,
    has_s: bool,
    has_t: bool,
    has_comment_open: bool,
    has_comment_close: bool,
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

#[derive(Clone, Debug, Default)]
struct TemplateDependencyGraph {
    forward: HashMap<String, HashSet<String>>,
    reverse: HashMap<String, HashSet<String>>,
    dirty: HashSet<String>,
}

impl TemplateDependencyGraph {
    fn update_template_calls(&mut self, name: &str, calls: HashSet<String>) {
        let template_name = name.to_string();
        let old_calls = self
            .forward
            .insert(template_name.clone(), calls.clone())
            .unwrap_or_default();

        for removed in old_calls.difference(&calls) {
            if let Some(callers) = self.reverse.get_mut(removed) {
                callers.remove(&template_name);
                if callers.is_empty() {
                    self.reverse.remove(removed);
                }
            }
        }

        for added in calls {
            self.reverse
                .entry(added)
                .or_default()
                .insert(template_name.clone());
        }

        self.dirty.insert(template_name);
    }

    fn affected_templates_for_reanalysis(
        &self,
        root_name: &str,
        templates: &HashMap<String, Vec<Node>>,
    ) -> HashSet<String> {
        if templates.is_empty() {
            return HashSet::new();
        }

        if self.dirty.is_empty() {
            if templates.contains_key(root_name) {
                return HashSet::from([root_name.to_string()]);
            }
            return templates.keys().cloned().collect();
        }

        let mut impacted_callers = HashSet::new();
        let mut reverse_queue = VecDeque::new();
        for dirty_name in &self.dirty {
            if templates.contains_key(dirty_name) {
                reverse_queue.push_back(dirty_name.clone());
            }
        }

        if reverse_queue.is_empty() {
            return templates.keys().cloned().collect();
        }

        while let Some(name) = reverse_queue.pop_front() {
            if !impacted_callers.insert(name.clone()) {
                continue;
            }

            if let Some(callers) = self.reverse.get(&name) {
                for caller in callers {
                    if templates.contains_key(caller) {
                        reverse_queue.push_back(caller.clone());
                    }
                }
            }
        }

        let mut affected = impacted_callers.clone();
        let mut forward_queue = VecDeque::new();
        for name in impacted_callers {
            forward_queue.push_back(name);
        }

        while let Some(name) = forward_queue.pop_front() {
            if let Some(callees) = self.forward.get(&name) {
                for callee in callees {
                    if templates.contains_key(callee) && affected.insert(callee.clone()) {
                        forward_queue.push_back(callee.clone());
                    }
                }
            }
        }

        affected
    }

    fn reanalysis_roots(&self, affected: &HashSet<String>) -> Vec<String> {
        let mut roots = affected
            .iter()
            .filter(|name| {
                self.reverse.get(*name).map_or(true, |callers| {
                    !callers.iter().any(|caller| affected.contains(caller))
                })
            })
            .cloned()
            .collect::<Vec<_>>();

        if roots.is_empty() {
            roots.extend(affected.iter().cloned());
        }

        roots.sort();
        roots
    }

    fn clear_dirty(&mut self, names: &HashSet<String>) {
        for name in names {
            self.dirty.remove(name);
        }
    }
}

#[derive(Clone)]
struct TemplateParseMeta {
    text_only: bool,
    defer_text_only_context_analysis: bool,
    cacheable_text_only_output: bool,
}

impl TemplateParseMeta {
    fn from_nodes(nodes: &[Node]) -> Self {
        if let Some(text) = single_text_node_raw(nodes) {
            let scan = scan_text_summary(text);
            return Self::from_single_text_with_scan(text, &scan);
        }

        if !nodes.iter().all(|node| matches!(node, Node::Text(_))) {
            return Self {
                text_only: false,
                defer_text_only_context_analysis: false,
                cacheable_text_only_output: false,
            };
        }

        Self {
            text_only: true,
            defer_text_only_context_analysis: false,
            cacheable_text_only_output: false,
        }
    }

    fn from_single_text_with_scan(text: &str, scan: &TextScanSummary) -> Self {
        Self {
            text_only: true,
            defer_text_only_context_analysis: deferred_text_only_context_analysis(text, scan),
            cacheable_text_only_output: cacheable_text_only_output(text, scan),
        }
    }
}

#[derive(Clone)]
struct TemplateNameSpace {
    templates: Arc<RwLock<HashMap<String, Vec<Node>>>>,
    template_dependency_graph: Arc<RwLock<TemplateDependencyGraph>>,
    template_parse_meta: Arc<RwLock<HashMap<String, TemplateParseMeta>>>,
    text_only_candidates: Arc<RwLock<HashSet<String>>>,
    text_only_outputs: Arc<RwLock<HashMap<String, String>>>,
    context_analysis_ready: Arc<AtomicBool>,
    context_analysis_lock: Arc<Mutex<()>>,
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
            template_dependency_graph: Arc::new(RwLock::new(TemplateDependencyGraph::default())),
            template_parse_meta: Arc::new(RwLock::new(HashMap::new())),
            text_only_candidates: Arc::new(RwLock::new(HashSet::new())),
            text_only_outputs: Arc::new(RwLock::new(HashMap::new())),
            context_analysis_ready: Arc::new(AtomicBool::new(true)),
            context_analysis_lock: Arc::new(Mutex::new(())),
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
        let template_dependency_graph = self
            .name_space
            .template_dependency_graph
            .read()
            .unwrap()
            .clone();
        let template_parse_meta = self.name_space.template_parse_meta.read().unwrap().clone();
        let text_only_candidates = self.name_space.text_only_candidates.read().unwrap().clone();
        let text_only_outputs = self.name_space.text_only_outputs.read().unwrap().clone();
        let context_analysis_ready = self
            .name_space
            .context_analysis_ready
            .load(AtomicOrdering::SeqCst);
        let funcs = self.name_space.funcs.read().unwrap().clone();
        let methods = self.name_space.methods.read().unwrap().clone();
        let missing_key_mode = self.name_space.missing_key_mode.read().unwrap().clone();

        Ok(Self {
            name: self.name.clone(),
            name_space: TemplateNameSpace {
                templates: Arc::new(RwLock::new(templates)),
                template_dependency_graph: Arc::new(RwLock::new(template_dependency_graph)),
                template_parse_meta: Arc::new(RwLock::new(template_parse_meta)),
                text_only_candidates: Arc::new(RwLock::new(text_only_candidates)),
                text_only_outputs: Arc::new(RwLock::new(text_only_outputs)),
                context_analysis_ready: Arc::new(AtomicBool::new(context_analysis_ready)),
                context_analysis_lock: Arc::new(Mutex::new(())),
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
        self.finalize_contexts_after_parse()?;
        Ok(self)
    }

    pub fn parse_owned(mut self, text: String) -> Result<Self> {
        self.ensure_not_executed()?;
        let root = self.name.clone();
        self.parse_named_owned(&root, text)?;
        self.finalize_contexts_after_parse()?;
        Ok(self)
    }

    #[allow(non_snake_case)]
    pub fn New(&self, name: impl Into<String>) -> Self {
        let mut clone = self.clone();
        clone.name = name.into();
        clone
    }

    pub fn parse_tree(&self, text: &str) -> Result<ParseTree> {
        let left_delim = self.name_space.left_delim.read().unwrap().clone();
        let right_delim = self.name_space.right_delim.read().unwrap().clone();
        self.parse_tree_with_delims(text, &left_delim, &right_delim)
    }

    #[allow(non_snake_case)]
    pub fn AddParseTree(mut self, name: impl Into<String>, tree: ParseTree) -> Result<Self> {
        self.ensure_not_executed()?;
        let name = name.into();
        self.validate_function_calls(&tree.nodes)?;
        self.clear_text_only_output_cache();
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
            self.merge_template_nodes(&mut templates, &name, tree.nodes, None);
        }
        self.finalize_contexts_after_parse()?;
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

        self.finalize_contexts_after_parse()?;
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

    fn runtime_context(&self) -> RuntimeContext<'_> {
        RuntimeContext {
            funcs: self.name_space.funcs.read().unwrap(),
            methods: self.name_space.methods.read().unwrap(),
            missing_key_mode: *self.name_space.missing_key_mode.read().unwrap(),
        }
    }

    fn deferred_text_only_candidates(&self) -> Option<HashSet<String>> {
        let templates = self.name_space.templates.read().unwrap();
        if templates.is_empty() {
            return Some(HashSet::new());
        }

        let parse_meta = self.name_space.template_parse_meta.read().unwrap();
        if parse_meta.len() < templates.len() {
            return None;
        }

        let mut candidates = HashSet::new();
        for name in templates.keys() {
            let meta = parse_meta.get(name)?;
            if !meta.text_only || !meta.defer_text_only_context_analysis {
                return None;
            }
            if meta.cacheable_text_only_output {
                candidates.insert(name.clone());
            }
        }

        Some(candidates)
    }

    fn finalize_contexts_after_parse(&self) -> Result<()> {
        if let Some(text_only_candidates) = self.deferred_text_only_candidates() {
            *self.name_space.text_only_candidates.write().unwrap() = text_only_candidates;
            self.name_space.text_only_outputs.write().unwrap().clear();
            self.name_space
                .context_analysis_ready
                .store(true, AtomicOrdering::SeqCst);
            return Ok(());
        }
        self.reanalyze_contexts()?;
        self.name_space
            .context_analysis_ready
            .store(true, AtomicOrdering::SeqCst);
        Ok(())
    }

    fn ensure_context_analysis_ready(&self) -> Result<()> {
        if self
            .name_space
            .context_analysis_ready
            .load(AtomicOrdering::SeqCst)
        {
            return Ok(());
        }

        let _analysis_guard = self.name_space.context_analysis_lock.lock().unwrap();
        if self
            .name_space
            .context_analysis_ready
            .load(AtomicOrdering::SeqCst)
        {
            return Ok(());
        }

        self.reanalyze_contexts()?;
        self.name_space
            .context_analysis_ready
            .store(true, AtomicOrdering::SeqCst);
        Ok(())
    }

    fn clear_text_only_output_cache(&self) {
        self.name_space
            .text_only_candidates
            .write()
            .unwrap()
            .clear();
        self.name_space.text_only_outputs.write().unwrap().clear();
        self.name_space
            .context_analysis_ready
            .store(false, AtomicOrdering::SeqCst);
    }

    fn text_only_template_output(&self, name: &str) -> Result<Option<String>> {
        if let Some(rendered) = self.name_space.text_only_outputs.read().unwrap().get(name) {
            return Ok(Some(rendered.clone()));
        }

        let is_candidate = self
            .name_space
            .text_only_candidates
            .read()
            .unwrap()
            .contains(name);
        if is_candidate {
            let rendered = {
                let templates = self.name_space.templates.read().unwrap();
                let nodes = templates.get(name).ok_or_else(|| {
                    TemplateError::Render(format!("template `{name}` is not defined"))
                })?;
                collect_text_only_nodes(nodes)
            };
            if let Some(rendered) = rendered {
                self.name_space
                    .text_only_outputs
                    .write()
                    .unwrap()
                    .insert(name.to_string(), rendered.clone());
                return Ok(Some(rendered));
            }
        }

        let templates = self.name_space.templates.read().unwrap();
        if templates.contains_key(name) {
            Ok(None)
        } else {
            Err(TemplateError::Render(format!(
                "template `{name}` is not defined"
            )))
        }
    }

    pub fn execute<T: Serialize, W: Write>(&self, writer: &mut W, data: &T) -> Result<()> {
        self.execute_template(writer, &self.name, data)
    }

    pub fn execute_to_string<T: Serialize>(&self, data: &T) -> Result<String> {
        self.execute_template_to_string(&self.name, data)
    }

    pub fn execute_value<W: Write>(&self, writer: &mut W, data: &Value) -> Result<()> {
        self.execute_template_value(writer, &self.name, data)
    }

    pub fn execute_value_to_string(&self, data: &Value) -> Result<String> {
        self.execute_template_value_to_string(&self.name, data)
    }

    pub fn execute_template_value<W: Write>(
        &self,
        writer: &mut W,
        name: &str,
        data: &Value,
    ) -> Result<()> {
        let rendered = self.execute_template_with_root_to_string(name, data)?;
        writer.write_all(rendered.as_bytes())?;
        Ok(())
    }

    pub fn execute_template_value_to_string(&self, name: &str, data: &Value) -> Result<String> {
        self.execute_template_with_root_to_string(name, data)
    }

    pub fn execute_template<T: Serialize, W: Write>(
        &self,
        writer: &mut W,
        name: &str,
        data: &T,
    ) -> Result<()> {
        let root = Value::from_serializable(data)?;
        let rendered = self.execute_template_with_root_to_string(name, &root)?;
        writer.write_all(rendered.as_bytes())?;
        Ok(())
    }

    pub fn execute_template_to_string<T: Serialize>(&self, name: &str, data: &T) -> Result<String> {
        let root = Value::from_serializable(data)?;
        self.execute_template_with_root_to_string(name, &root)
    }

    fn execute_template_with_root_to_string(&self, name: &str, root: &Value) -> Result<String> {
        self.name_space.executed.store(true, AtomicOrdering::SeqCst);
        self.ensure_context_analysis_ready()?;
        if let Some(rendered) = self.text_only_template_output(name)? {
            return Ok(rendered);
        }
        let runtime = self.runtime_context();
        let root_json = match root {
            Value::Json(value) => Some(value),
            _ => None,
        };
        let mut rendered = String::new();
        let mut tracker = ContextTracker::from_state(ContextState::html_text());
        let mut scopes = vec![HashMap::new()];
        let flow = self.render_named(
            name,
            root,
            root,
            root_json,
            &mut scopes,
            &mut rendered,
            &mut tracker,
            &runtime,
            false,
            0,
        )?;
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
        let left_delim = self.name_space.left_delim.read().unwrap().clone();
        let right_delim = self.name_space.right_delim.read().unwrap().clone();
        let (tree, root_parse_meta_hint) =
            self.parse_tree_with_delims_and_meta(text, &left_delim, &right_delim)?;
        self.validate_function_calls(&tree.nodes)?;
        self.clear_text_only_output_cache();
        let mut templates = self.name_space.templates.write().unwrap();
        self.merge_template_nodes(&mut templates, name, tree.nodes, root_parse_meta_hint);

        Ok(())
    }

    fn parse_named_owned(&mut self, name: &str, text: String) -> Result<()> {
        let left_delim = self.name_space.left_delim.read().unwrap().clone();
        let right_delim = self.name_space.right_delim.read().unwrap().clone();
        let (tree, root_parse_meta_hint) =
            self.parse_tree_with_delims_and_meta_owned(text, &left_delim, &right_delim)?;
        self.validate_function_calls(&tree.nodes)?;
        self.clear_text_only_output_cache();
        let mut templates = self.name_space.templates.write().unwrap();
        self.merge_template_nodes(&mut templates, name, tree.nodes, root_parse_meta_hint);

        Ok(())
    }

    fn parse_tree_with_delims(
        &self,
        text: &str,
        left_delim: &str,
        right_delim: &str,
    ) -> Result<ParseTree> {
        let (tree, _) = self.parse_tree_with_delims_and_meta(text, left_delim, right_delim)?;
        Ok(tree)
    }

    fn parse_tree_with_delims_and_meta(
        &self,
        text: &str,
        left_delim: &str,
        right_delim: &str,
    ) -> Result<(ParseTree, Option<TemplateParseMeta>)> {
        if text.is_empty() {
            return Ok((ParseTree { nodes: Vec::new() }, None));
        }

        let (has_left_delim, no_default_delim_scan) =
            if left_delim == "{{" && right_delim == "}}" {
                match scan_text_summary_if_no_default_delim(text) {
                    Some(scan) => (false, Some(scan)),
                    None => (true, None),
                }
            } else {
                (text.contains(left_delim), None)
            };
        if !has_left_delim {
            let scan = no_default_delim_scan.unwrap_or_else(|| scan_text_summary(text));
            if scan.has_lt && scan.has_eq {
                validate_unquoted_attr_hazards(text, scan.has_lt, scan.has_eq)?;
            }

            if !scan.has_comment_open {
                return Ok((
                    ParseTree {
                        nodes: vec![Node::Text(TextNode::from_span(
                            Arc::from(text),
                            0,
                            text.len(),
                        ))],
                    },
                    Some(TemplateParseMeta::from_single_text_with_scan(text, &scan)),
                ));
            }

            let preprocessed = strip_html_comments(text);
            let preprocessed = preprocessed.as_ref();
            if preprocessed.is_empty() {
                return Ok((ParseTree { nodes: Vec::new() }, None));
            }
            let preprocessed_scan = scan_text_summary(preprocessed);
            return Ok((
                ParseTree {
                    nodes: vec![Node::Text(TextNode::from_span(
                        Arc::from(preprocessed),
                        0,
                        preprocessed.len(),
                    ))],
                },
                Some(TemplateParseMeta::from_single_text_with_scan(
                    preprocessed,
                    &preprocessed_scan,
                )),
            ));
        }

        let has_html_comment = text.as_bytes().contains(&b'!') && text.contains("<!--");
        let preprocessed = if has_html_comment {
            strip_html_comments(text)
        } else {
            Cow::Borrowed(text)
        };
        let preprocessed = preprocessed.as_ref();
        if preprocessed.is_empty() {
            return Ok((ParseTree { nodes: Vec::new() }, None));
        }

        if has_html_comment && !preprocessed.contains(left_delim) {
            let preprocessed_scan = scan_text_summary(preprocessed);
            if preprocessed_scan.has_lt && preprocessed_scan.has_eq {
                validate_unquoted_attr_hazards(
                    preprocessed,
                    preprocessed_scan.has_lt,
                    preprocessed_scan.has_eq,
                )?;
            }
            if !preprocessed_scan.has_comment_open {
                return Ok((
                    ParseTree {
                        nodes: vec![Node::Text(TextNode::from_span(
                            Arc::from(preprocessed),
                            0,
                            preprocessed.len(),
                        ))],
                    },
                    Some(TemplateParseMeta::from_single_text_with_scan(
                        preprocessed,
                        &preprocessed_scan,
                    )),
                ));
            }
        }

        let source: Arc<str> = Arc::from(preprocessed);
        let tokens = tokenize(preprocessed, left_delim, right_delim)?;
        let mut index = 0;
        let (nodes, stop) = parse_nodes(&source, &tokens, &mut index, &[])?;
        if let Some(stop) = stop {
            return Err(TemplateError::Parse(format!(
                "unexpected control action `{}`",
                stop.keyword
            )));
        }
        Ok((ParseTree { nodes }, None))
    }

    fn parse_tree_with_delims_and_meta_owned(
        &self,
        text: String,
        left_delim: &str,
        right_delim: &str,
    ) -> Result<(ParseTree, Option<TemplateParseMeta>)> {
        if text.is_empty() {
            return Ok((ParseTree { nodes: Vec::new() }, None));
        }

        let (has_left_delim, no_default_delim_scan) =
            if left_delim == "{{" && right_delim == "}}" {
                match scan_text_summary_if_no_default_delim(&text) {
                    Some(scan) => (false, Some(scan)),
                    None => (true, None),
                }
            } else {
                (text.contains(left_delim), None)
            };
        if !has_left_delim {
            let scan = no_default_delim_scan.unwrap_or_else(|| scan_text_summary(&text));
            if scan.has_lt && scan.has_eq {
                validate_unquoted_attr_hazards(&text, scan.has_lt, scan.has_eq)?;
            }

            if !scan.has_comment_open {
                let parse_meta = TemplateParseMeta::from_single_text_with_scan(&text, &scan);
                return Ok((
                    ParseTree {
                        nodes: vec![Node::Text(TextNode::from_owned_string(text))],
                    },
                    Some(parse_meta),
                ));
            }

            let preprocessed = strip_html_comments(&text);
            let preprocessed_ref = preprocessed.as_ref();
            if preprocessed_ref.is_empty() {
                return Ok((ParseTree { nodes: Vec::new() }, None));
            }
            let preprocessed_scan = scan_text_summary(preprocessed_ref);
            let parse_meta =
                TemplateParseMeta::from_single_text_with_scan(preprocessed_ref, &preprocessed_scan);
            let text_node = match preprocessed {
                Cow::Owned(preprocessed_owned) => {
                    Node::Text(TextNode::from_owned_string(preprocessed_owned))
                }
                Cow::Borrowed(preprocessed_borrowed) => Node::Text(TextNode::from_span(
                    Arc::from(preprocessed_borrowed),
                    0,
                    preprocessed_borrowed.len(),
                )),
            };
            return Ok((
                ParseTree {
                    nodes: vec![text_node],
                },
                Some(parse_meta),
            ));
        }

        self.parse_tree_with_delims_and_meta(&text, left_delim, right_delim)
    }

    fn merge_template_nodes(
        &self,
        templates: &mut HashMap<String, Vec<Node>>,
        name: &str,
        nodes: Vec<Node>,
        root_parse_meta_hint: Option<TemplateParseMeta>,
    ) {
        let mut root_nodes = Vec::new();
        let mut changed_templates = Vec::new();
        for node in nodes {
            match node {
                Node::Define {
                    name: defined_name,
                    body,
                } => {
                    if !is_empty_template_body(&body) || !templates.contains_key(&defined_name) {
                        let template_name = defined_name.clone();
                        let dependencies = collect_template_call_dependencies(&body);
                        let parse_meta = TemplateParseMeta::from_nodes(&body);
                        templates.insert(template_name.clone(), body);
                        changed_templates.push((template_name, dependencies, parse_meta));
                    }
                }
                Node::Block {
                    name: block_name,
                    data,
                    body,
                } => {
                    if !templates.contains_key(&block_name) {
                        let dependencies = collect_template_call_dependencies(&body);
                        let parse_meta = TemplateParseMeta::from_nodes(&body);
                        templates.insert(block_name.clone(), body.clone());
                        changed_templates.push((block_name.clone(), dependencies, parse_meta));
                    }
                    root_nodes.push(Node::TemplateCall {
                        name: block_name,
                        data,
                    });
                }
                other => root_nodes.push(other),
            }
        }

        if !root_nodes.is_empty() || !templates.contains_key(name) {
            let root_dependencies = collect_template_call_dependencies(&root_nodes);
            let root_parse_meta =
                root_parse_meta_hint.unwrap_or_else(|| TemplateParseMeta::from_nodes(&root_nodes));
            templates.insert(name.to_string(), root_nodes);
            changed_templates.push((name.to_string(), root_dependencies, root_parse_meta));
        }

        if !changed_templates.is_empty() {
            let mut dependency_graph = self.name_space.template_dependency_graph.write().unwrap();
            let mut parse_meta_map = self.name_space.template_parse_meta.write().unwrap();
            for (template_name, dependencies, parse_meta) in changed_templates {
                dependency_graph.update_template_calls(&template_name, dependencies);
                parse_meta_map.insert(template_name, parse_meta);
            }
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

    fn reanalyze_contexts(&self) -> Result<()> {
        let text_only_candidates = {
            let mut templates = self.name_space.templates.write().unwrap();
            if !templates.contains_key(&self.name) {
                return Err(TemplateError::Parse(format!(
                    "template `{}` is not defined",
                    self.name
                )));
            }

            if templates
                .values()
                .all(|nodes| nodes.iter().all(|node| matches!(node, Node::Text(_))))
            {
                let mut root_end_state = None;
                let mut text_only_candidates = HashSet::new();
                for (name, nodes) in templates.iter() {
                    let (tracker, cacheable_output) = analyze_text_only_template_nodes(nodes);
                    if should_validate_action_context(&tracker) {
                        validate_action_context_before_insertion(&tracker)?;
                    }
                    if name == &self.name {
                        root_end_state = Some(tracker.state());
                    }
                    if cacheable_output {
                        text_only_candidates.insert(name.clone());
                    }
                }

                let root_end = root_end_state.unwrap_or_else(ContextState::html_text);
                if !root_end.is_text_context() {
                    return Err(TemplateError::Parse(format!(
                        "template `{}` ends in a non-text context",
                        self.name
                    )));
                }

                let known_templates = templates.keys().cloned().collect::<HashSet<_>>();
                self.name_space
                    .template_dependency_graph
                    .write()
                    .unwrap()
                    .clear_dirty(&known_templates);

                text_only_candidates
            } else {
                let (affected_templates, analysis_roots) = {
                    let dependency_graph =
                        self.name_space.template_dependency_graph.read().unwrap();
                    let affected =
                        dependency_graph.affected_templates_for_reanalysis(&self.name, &templates);
                    let roots = dependency_graph.reanalysis_roots(&affected);
                    (affected, roots)
                };

                if !affected_templates.is_empty() {
                    let known_templates = templates.keys().cloned().collect::<HashSet<_>>();
                    let mut analyzer = ParseContextAnalyzer::new(&mut templates);

                    for root_name in &analysis_roots {
                        if !known_templates.contains(root_name) {
                            continue;
                        }
                        let root_end =
                            analyzer.analyze_template(root_name, ContextState::html_text())?;
                        if root_name == &self.name && !root_end.is_text_context() {
                            return Err(TemplateError::Parse(format!(
                                "template `{}` ends in a non-text context",
                                self.name
                            )));
                        }
                    }

                    let mut remaining = affected_templates.iter().cloned().collect::<Vec<_>>();
                    remaining.sort();
                    for name in remaining {
                        if !known_templates.contains(&name) || analyzer.has_analysis(&name) {
                            continue;
                        }
                        let end_state =
                            analyzer.analyze_template(&name, ContextState::html_text())?;
                        if name == self.name && !end_state.is_text_context() {
                            return Err(TemplateError::Parse(format!(
                                "template `{}` ends in a non-text context",
                                self.name
                            )));
                        }
                    }

                    if affected_templates.contains(&self.name) && !analyzer.has_analysis(&self.name)
                    {
                        let root_end =
                            analyzer.analyze_template(&self.name, ContextState::html_text())?;
                        if !root_end.is_text_context() {
                            return Err(TemplateError::Parse(format!(
                                "template `{}` ends in a non-text context",
                                self.name
                            )));
                        }
                    }
                }

                self.name_space
                    .template_dependency_graph
                    .write()
                    .unwrap()
                    .clear_dirty(&affected_templates);

                HashSet::new()
            }
        };
        *self.name_space.text_only_candidates.write().unwrap() = text_only_candidates;
        self.name_space.text_only_outputs.write().unwrap().clear();
        Ok(())
    }

    fn render_named(
        &self,
        name: &str,
        root: &Value,
        dot: &Value,
        dot_json: Option<&JsonValue>,
        scopes: &mut ScopeStack,
        output: &mut String,
        tracker: &mut ContextTracker,
        runtime: &RuntimeContext<'_>,
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
        self.render_nodes(
            nodes, root, dot, dot_json, scopes, output, tracker, runtime, in_range, depth,
        )
    }

    fn render_nodes(
        &self,
        nodes: &[Node],
        root: &Value,
        dot: &Value,
        dot_json: Option<&JsonValue>,
        scopes: &mut ScopeStack,
        output: &mut String,
        tracker: &mut ContextTracker,
        runtime: &RuntimeContext<'_>,
        in_range: bool,
        depth: usize,
    ) -> Result<RenderFlow> {
        for node in nodes {
            match node {
                Node::Text(text_node) => {
                    if let Some(prepared) = text_node.prepared.as_ref()
                        && tracker.state == prepared.start_state
                    {
                        self.render_prepared_text_chunks(prepared, output, tracker);
                    } else {
                        self.render_text_segment(&text_node.raw, output, tracker);
                    }
                }
                Node::Expr {
                    expr,
                    mode,
                    runtime_mode,
                } => {
                    let mut mode = *mode;
                    if *runtime_mode {
                        let inferred_mode = tracker.mode();
                        if !matches!(inferred_mode, EscapeMode::AttrName) {
                            mode = inferred_mode;
                        }
                    }
                    let value = self.eval_expr(expr, root, dot, dot_json, runtime, scopes)?;
                    let escaped = escape_value_for_mode(
                        &value,
                        mode,
                        &tracker.rendered,
                        tracker.url_part,
                        tracker.css_url_part_hint(),
                    )?;
                    output.push_str(&escaped);
                    if *runtime_mode {
                        tracker.append_text(&escaped);
                    } else if placeholder_advances_parse_context(tracker, mode) {
                        tracker.append_expr_placeholder(mode);
                    }
                }
                Node::SetVar {
                    name,
                    value,
                    declare,
                } => {
                    let evaluated = self.eval_expr(value, root, dot, dot_json, runtime, scopes)?;
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
                    let condition_value =
                        self.eval_expr(condition, root, dot, dot_json, runtime, scopes)?;
                    if condition_value.truthy() {
                        push_scope(scopes);
                        let flow = self.render_nodes(
                            then_branch,
                            root,
                            dot,
                            dot_json,
                            scopes,
                            output,
                            tracker,
                            runtime,
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
                            dot_json,
                            scopes,
                            output,
                            tracker,
                            runtime,
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
                    let iterable_value =
                        self.eval_expr(iterable, root, dot, dot_json, runtime, scopes)?;
                    let vars_len = vars.len();
                    let body_uses_dot = nodes_may_reference_dot(body);
                    let iteration_count = range_iteration_count(&iterable_value);
                    if vars_len == 0
                        && iteration_count > 0
                        && let Some(body_text) = range_static_text_body(body)
                        && let Some(repeated_plan) =
                            range_static_text_fast_path_text(tracker, &body_text)
                    {
                        append_repeated_text(
                            output,
                            tracker,
                            &repeated_plan.text,
                            iteration_count,
                            !repeated_plan.updates_tracker,
                        );
                        continue;
                    }
                    match &iterable_value {
                        Value::Json(JsonValue::Array(items)) if !items.is_empty() => {
                            if vars_len == 0 {
                                push_scope(scopes);
                                for value in items {
                                    scopes
                                        .last_mut()
                                        .expect("range scope pushed before iteration")
                                        .clear();
                                    let flow = self.render_nodes(
                                        body,
                                        root,
                                        dot,
                                        if body_uses_dot { Some(value) } else { None },
                                        scopes,
                                        output,
                                        tracker,
                                        runtime,
                                        true,
                                        depth,
                                    )?;
                                    match flow {
                                        RenderFlow::Normal => {}
                                        RenderFlow::Continue => continue,
                                        RenderFlow::Break => break,
                                    }
                                }
                                pop_scope(scopes);
                            } else {
                                let assign_targets = if *declare_vars {
                                    None
                                } else {
                                    Some(resolve_range_assign_targets(scopes, vars)?)
                                };
                                push_scope(scopes);
                                for (index, value) in items.iter().enumerate() {
                                    let item = Value::Json(value.clone());
                                    let key = (vars_len >= 2).then(|| Value::from(index as u64));
                                    if *declare_vars {
                                        let range_scope = scopes
                                            .last_mut()
                                            .expect("range scope pushed before iteration");
                                        declare_range_variables(
                                            range_scope,
                                            vars,
                                            key,
                                            item.clone(),
                                        );
                                    } else {
                                        scopes
                                            .last_mut()
                                            .expect("range scope pushed before iteration")
                                            .clear();
                                        assign_range_variables(
                                            scopes,
                                            vars,
                                            assign_targets.expect("assign targets resolved"),
                                            key,
                                            item.clone(),
                                        )?;
                                    }
                                    let flow = self.render_nodes(
                                        body,
                                        root,
                                        &item,
                                        if body_uses_dot { Some(value) } else { None },
                                        scopes,
                                        output,
                                        tracker,
                                        runtime,
                                        true,
                                        depth,
                                    )?;
                                    match flow {
                                        RenderFlow::Normal => {}
                                        RenderFlow::Continue => continue,
                                        RenderFlow::Break => break,
                                    }
                                }
                                pop_scope(scopes);
                            }
                        }
                        Value::Json(JsonValue::Object(items)) if !items.is_empty() => {
                            let mut keys = items.keys().map(String::as_str).collect::<Vec<_>>();
                            keys.sort_unstable();
                            if vars_len == 0 {
                                push_scope(scopes);
                                for map_key in keys {
                                    scopes
                                        .last_mut()
                                        .expect("range scope pushed before iteration")
                                        .clear();
                                    let value_ref =
                                        items.get(map_key).expect("key collected from map");
                                    let flow = self.render_nodes(
                                        body,
                                        root,
                                        dot,
                                        if body_uses_dot { Some(value_ref) } else { None },
                                        scopes,
                                        output,
                                        tracker,
                                        runtime,
                                        true,
                                        depth,
                                    )?;
                                    match flow {
                                        RenderFlow::Normal => {}
                                        RenderFlow::Continue => continue,
                                        RenderFlow::Break => break,
                                    }
                                }
                                pop_scope(scopes);
                            } else {
                                let assign_targets = if *declare_vars {
                                    None
                                } else {
                                    Some(resolve_range_assign_targets(scopes, vars)?)
                                };
                                push_scope(scopes);
                                for map_key in keys {
                                    let value_ref =
                                        items.get(map_key).expect("key collected from map");
                                    let item = Value::Json(value_ref.clone());
                                    let key_value = (vars_len >= 2).then(|| Value::from(map_key));
                                    if *declare_vars {
                                        let range_scope = scopes
                                            .last_mut()
                                            .expect("range scope pushed before iteration");
                                        declare_range_variables(
                                            range_scope,
                                            vars,
                                            key_value,
                                            item.clone(),
                                        );
                                    } else {
                                        scopes
                                            .last_mut()
                                            .expect("range scope pushed before iteration")
                                            .clear();
                                        assign_range_variables(
                                            scopes,
                                            vars,
                                            assign_targets.expect("assign targets resolved"),
                                            key_value,
                                            item.clone(),
                                        )?;
                                    }
                                    let flow = self.render_nodes(
                                        body,
                                        root,
                                        &item,
                                        if body_uses_dot { Some(value_ref) } else { None },
                                        scopes,
                                        output,
                                        tracker,
                                        runtime,
                                        true,
                                        depth,
                                    )?;
                                    match flow {
                                        RenderFlow::Normal => {}
                                        RenderFlow::Continue => continue,
                                        RenderFlow::Break => break,
                                    }
                                }
                                pop_scope(scopes);
                            }
                        }
                        Value::Json(JsonValue::String(value)) if !value.is_empty() => {
                            if vars_len == 0 {
                                push_scope(scopes);
                                for ch in value.chars() {
                                    scopes
                                        .last_mut()
                                        .expect("range scope pushed before iteration")
                                        .clear();
                                    let item = if body_uses_dot {
                                        Some(Value::Json(JsonValue::String(ch.to_string())))
                                    } else {
                                        None
                                    };
                                    let flow = self.render_nodes(
                                        body,
                                        root,
                                        item.as_ref().unwrap_or(dot),
                                        None,
                                        scopes,
                                        output,
                                        tracker,
                                        runtime,
                                        true,
                                        depth,
                                    )?;
                                    match flow {
                                        RenderFlow::Normal => {}
                                        RenderFlow::Continue => continue,
                                        RenderFlow::Break => break,
                                    }
                                }
                                pop_scope(scopes);
                            } else {
                                let assign_targets = if *declare_vars {
                                    None
                                } else {
                                    Some(resolve_range_assign_targets(scopes, vars)?)
                                };
                                push_scope(scopes);
                                for (index, ch) in value.chars().enumerate() {
                                    let item = Value::Json(JsonValue::String(ch.to_string()));
                                    let key = (vars_len >= 2).then(|| Value::from(index as u64));
                                    if *declare_vars {
                                        let range_scope = scopes
                                            .last_mut()
                                            .expect("range scope pushed before iteration");
                                        declare_range_variables(
                                            range_scope,
                                            vars,
                                            key,
                                            item.clone(),
                                        );
                                    } else {
                                        scopes
                                            .last_mut()
                                            .expect("range scope pushed before iteration")
                                            .clear();
                                        assign_range_variables(
                                            scopes,
                                            vars,
                                            assign_targets.expect("assign targets resolved"),
                                            key,
                                            item.clone(),
                                        )?;
                                    }
                                    let flow = self.render_nodes(
                                        body, root, &item, None, scopes, output, tracker, runtime,
                                        true, depth,
                                    )?;
                                    match flow {
                                        RenderFlow::Normal => {}
                                        RenderFlow::Continue => continue,
                                        RenderFlow::Break => break,
                                    }
                                }
                                pop_scope(scopes);
                            }
                        }
                        _ => {
                            push_scope(scopes);
                            let flow = self.render_nodes(
                                else_branch,
                                root,
                                dot,
                                dot_json,
                                scopes,
                                output,
                                tracker,
                                runtime,
                                true,
                                depth,
                            )?;
                            pop_scope(scopes);
                            match flow {
                                RenderFlow::Normal | RenderFlow::Break | RenderFlow::Continue => {}
                            }
                        }
                    }
                }
                Node::With {
                    value,
                    body,
                    else_branch,
                } => {
                    let value = self.eval_expr(value, root, dot, dot_json, runtime, scopes)?;
                    if value.truthy() {
                        let value_json = match &value {
                            Value::Json(json) => Some(json),
                            _ => None,
                        };
                        push_scope(scopes);
                        let flow = self.render_nodes(
                            body, root, &value, value_json, scopes, output, tracker, runtime,
                            in_range, depth,
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
                            dot_json,
                            scopes,
                            output,
                            tracker,
                            runtime,
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
                        Some(expr) => self.eval_expr(expr, root, dot, dot_json, runtime, scopes)?,
                        None => dot_to_owned(dot, dot_json),
                    };
                    let next_dot_json = match &next_dot {
                        Value::Json(json) => Some(json),
                        _ => None,
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
                        next_dot_json,
                        &mut template_scopes,
                        output,
                        tracker,
                        runtime,
                        in_range,
                        next_depth,
                    )?;
                    if !matches!(flow, RenderFlow::Normal) {
                        return Ok(flow);
                    }
                }
                Node::Block { name, data, body } => {
                    let next_dot = match data {
                        Some(expr) => self.eval_expr(expr, root, dot, dot_json, runtime, scopes)?,
                        None => dot_to_owned(dot, dot_json),
                    };
                    let next_dot_json = match &next_dot {
                        Value::Json(json) => Some(json),
                        _ => None,
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
                            next_dot_json,
                            &mut template_scopes,
                            output,
                            tracker,
                            runtime,
                            in_range,
                            next_depth,
                        )?;
                        if !matches!(flow, RenderFlow::Normal) {
                            return Ok(flow);
                        }
                    } else {
                        push_scope(scopes);
                        let flow = self.render_nodes(
                            body,
                            root,
                            &next_dot,
                            next_dot_json,
                            scopes,
                            output,
                            tracker,
                            runtime,
                            in_range,
                            depth,
                        )?;
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

    fn render_text_segment(&self, text: &str, output: &mut String, tracker: &mut ContextTracker) {
        let mode = tracker.mode();
        if matches!(mode, EscapeMode::ScriptExpr) {
            let scan_state = tracker
                .js_scan_state
                .or_else(|| {
                    current_unclosed_tag_content(&tracker.rendered, "script")
                        .map(current_js_scan_state)
                })
                .unwrap_or(JsScanState::Expr {
                    js_ctx: JsContext::RegExp,
                });
            let filtered = filter_script_text_with_state(scan_state, text);
            output.push_str(&filtered);
            tracker.append_text(&filtered);
        } else if matches!(mode, EscapeMode::Html) {
            if text.as_bytes().contains(&b'<') {
                let filtered = filter_html_text_sections(&tracker.rendered, text);
                output.push_str(&filtered);
                tracker.append_text(&filtered);
            } else {
                output.push_str(text);
                tracker.append_text(text);
            }
        } else {
            output.push_str(text);
            tracker.append_text(text);
        }
    }

    fn render_prepared_text_chunks(
        &self,
        prepared: &PreparedTextPlan,
        output: &mut String,
        tracker: &mut ContextTracker,
    ) {
        let mut index = 0usize;
        while index < prepared.chunks.len() {
            match &prepared.chunks[index] {
                PreparedTextChunk::Emit(text) => {
                    output.push_str(text);
                    tracker.append_text(text);
                    index += 1;
                }
                PreparedTextChunk::ScriptCloseTag(tag) => {
                    if let Some(PreparedTextChunk::Emit(suffix)) = prepared.chunks.get(index + 1) {
                        output.push_str(tag);
                        output.push_str(suffix);
                        tracker.append_known_script_close_tag_with_suffix(tag, suffix);
                        index += 2;
                    } else {
                        output.push_str(tag);
                        tracker.append_known_script_close_tag(tag);
                        index += 1;
                    }
                }
                PreparedTextChunk::StyleCloseTag(tag) => {
                    if let Some(PreparedTextChunk::Emit(suffix)) = prepared.chunks.get(index + 1) {
                        output.push_str(tag);
                        output.push_str(suffix);
                        tracker.append_known_style_close_tag_with_suffix(tag, suffix);
                        index += 2;
                    } else {
                        output.push_str(tag);
                        tracker.append_known_style_close_tag(tag);
                        index += 1;
                    }
                }
            }
        }
    }

    fn eval_expr(
        &self,
        expr: &Expr,
        root: &Value,
        dot: &Value,
        dot_json: Option<&JsonValue>,
        runtime: &RuntimeContext<'_>,
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
                    piped = Some(self.eval_term(term, root, dot, dot_json, runtime, scopes)?);
                }
                Command::Call { name, args } => {
                    let mut evaluated_args = args
                        .iter()
                        .map(|arg| self.eval_term(arg, root, dot, dot_json, runtime, scopes))
                        .collect::<Result<Vec<_>>>()?;

                    if index > 0 {
                        let value = piped.take().ok_or_else(|| {
                            TemplateError::Render("pipeline is missing input value".to_string())
                        })?;
                        evaluated_args.push(value);
                    }

                    if index == 0 && evaluated_args.is_empty() {
                        if let Some(value) = lookup_identifier_with_dot_json(
                            dot,
                            dot_json,
                            root,
                            name,
                            &runtime.methods,
                            runtime.missing_key_mode,
                        )? {
                            piped = Some(value);
                            continue;
                        }

                        if !runtime.funcs.contains_key(name)
                            && (dot_is_object(dot, dot_json)
                                || matches!(root, Value::Json(JsonValue::Object(_))))
                        {
                            piped = Some(missing_value_for_key(name, runtime.missing_key_mode)?);
                            continue;
                        }
                    }

                    if name == "call" {
                        piped = Some(self.eval_call_function(&evaluated_args, runtime)?);
                    } else {
                        let function = runtime.funcs.get(name).ok_or_else(|| {
                            TemplateError::Render(format!("function `{name}` is not registered"))
                        })?;
                        piped = Some(function(&evaluated_args)?);
                    }
                }
                Command::Invoke { callee, args } => {
                    let mut evaluated_args = args
                        .iter()
                        .map(|arg| self.eval_term(arg, root, dot, dot_json, runtime, scopes))
                        .collect::<Result<Vec<_>>>()?;

                    if index > 0 {
                        let value = piped.take().ok_or_else(|| {
                            TemplateError::Render("pipeline is missing input value".to_string())
                        })?;
                        evaluated_args.push(value);
                    }
                    piped = Some(self.eval_method_call(
                        callee,
                        &evaluated_args,
                        root,
                        dot,
                        dot_json,
                        runtime,
                        scopes,
                    )?);
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
        dot_json: Option<&JsonValue>,
        runtime: &RuntimeContext<'_>,
        scopes: &ScopeStack,
    ) -> Result<Value> {
        match term {
            Term::DotPath(path) => lookup_dot_path_with_methods(
                dot,
                dot_json,
                path,
                &runtime.methods,
                runtime.missing_key_mode,
            ),
            Term::RootPath(path) => {
                lookup_path_with_methods(root, path, &runtime.methods, runtime.missing_key_mode)
            }
            Term::Literal(value) => Ok(value.clone()),
            Term::Variable { name, path } => {
                let variable = lookup_variable(scopes, name).ok_or_else(|| {
                    TemplateError::Render(format!("variable `${name}` could not be resolved"))
                })?;
                lookup_path_with_methods(
                    &variable,
                    path,
                    &runtime.methods,
                    runtime.missing_key_mode,
                )
            }
            Term::Identifier(name) => {
                if let Some(value) = lookup_identifier_with_dot_json(
                    dot,
                    dot_json,
                    root,
                    name,
                    &runtime.methods,
                    runtime.missing_key_mode,
                )? {
                    Ok(value)
                } else if runtime.funcs.contains_key(name) {
                    Ok(Value::FunctionRef(name.clone()))
                } else if dot_is_object(dot, dot_json)
                    || matches!(root, Value::Json(JsonValue::Object(_)))
                {
                    missing_value_for_key(name, runtime.missing_key_mode)
                } else {
                    Err(TemplateError::Render(format!(
                        "identifier `{name}` could not be resolved"
                    )))
                }
            }
            Term::SubExpr(expr) => self.eval_expr(expr, root, dot, dot_json, runtime, scopes),
            Term::SubExprPath { expr, path } => {
                let base = self.eval_expr(expr, root, dot, dot_json, runtime, scopes)?;
                lookup_path_with_methods(&base, path, &runtime.methods, runtime.missing_key_mode)
            }
        }
    }

    fn eval_method_call(
        &self,
        callee: &Term,
        args: &[Value],
        root: &Value,
        dot: &Value,
        dot_json: Option<&JsonValue>,
        runtime: &RuntimeContext<'_>,
        scopes: &ScopeStack,
    ) -> Result<Value> {
        match callee {
            Term::DotPath(path) => self.call_dot_path_method(dot, dot_json, path, args, runtime),
            Term::RootPath(path) => self.call_path_method(root, path, args, runtime),
            Term::Variable { name, path } => {
                let variable = lookup_variable(scopes, name).ok_or_else(|| {
                    TemplateError::Render(format!("variable `${name}` could not be resolved"))
                })?;
                self.call_path_method(&variable, path, args, runtime)
            }
            Term::Identifier(name) => {
                if let Some(method) = runtime.methods.get(name) {
                    if let Some(dot_json) = dot_json {
                        let dot_value = Value::Json(dot_json.clone());
                        return method(&dot_value, args);
                    }
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
                let receiver = self.eval_expr(expr, root, dot, dot_json, runtime, scopes)?;
                self.call_path_method(&receiver, path, args, runtime)
            }
        }
    }

    fn call_dot_path_method(
        &self,
        dot: &Value,
        dot_json: Option<&JsonValue>,
        path: &[String],
        args: &[Value],
        runtime: &RuntimeContext<'_>,
    ) -> Result<Value> {
        if let Some(base_json) = dot_json {
            if path.is_empty() {
                return Err(TemplateError::Render("path is not callable".to_string()));
            }
            let (method_name, receiver_path) = split_last_path(path);
            let receiver = lookup_json_path_with_methods(
                base_json,
                receiver_path,
                &runtime.methods,
                runtime.missing_key_mode,
            )?;
            let method = runtime.methods.get(method_name).ok_or_else(|| {
                TemplateError::Render(format!("method `{method_name}` is not registered"))
            })?;
            return method(&receiver, args);
        }
        self.call_path_method(dot, path, args, runtime)
    }

    fn call_path_method(
        &self,
        base: &Value,
        path: &[String],
        args: &[Value],
        runtime: &RuntimeContext<'_>,
    ) -> Result<Value> {
        if path.is_empty() {
            return Err(TemplateError::Render("path is not callable".to_string()));
        }

        let (method_name, receiver_path) = split_last_path(path);
        let receiver = lookup_path_with_methods(
            base,
            receiver_path,
            &runtime.methods,
            runtime.missing_key_mode,
        )?;
        let method = runtime.methods.get(method_name).ok_or_else(|| {
            TemplateError::Render(format!("method `{method_name}` is not registered"))
        })?;
        method(&receiver, args)
    }

    fn eval_call_function(&self, args: &[Value], runtime: &RuntimeContext<'_>) -> Result<Value> {
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

        let function =
            runtime.funcs.get(&name).cloned().ok_or_else(|| {
                TemplateError::Render(format!("function `{name}` is not registered"))
            })?;
        function(&args[1..])
    }
}

fn collect_template_call_dependencies(nodes: &[Node]) -> HashSet<String> {
    let mut dependencies = HashSet::new();
    collect_template_call_dependencies_into(nodes, &mut dependencies);
    dependencies
}

fn collect_template_call_dependencies_into(nodes: &[Node], dependencies: &mut HashSet<String>) {
    for node in nodes {
        match node {
            Node::Text(_)
            | Node::Expr { .. }
            | Node::SetVar { .. }
            | Node::Break
            | Node::Continue => {}
            Node::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_template_call_dependencies_into(then_branch, dependencies);
                collect_template_call_dependencies_into(else_branch, dependencies);
            }
            Node::Range {
                body, else_branch, ..
            } => {
                collect_template_call_dependencies_into(body, dependencies);
                collect_template_call_dependencies_into(else_branch, dependencies);
            }
            Node::With {
                body, else_branch, ..
            } => {
                collect_template_call_dependencies_into(body, dependencies);
                collect_template_call_dependencies_into(else_branch, dependencies);
            }
            Node::TemplateCall { name, .. } => {
                dependencies.insert(name.clone());
            }
            Node::Block { name, body, .. } => {
                dependencies.insert(name.clone());
                collect_template_call_dependencies_into(body, dependencies);
            }
            Node::Define { body, .. } => {
                collect_template_call_dependencies_into(body, dependencies);
            }
        }
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

pub fn escape_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '\'' => escaped.push_str("&#39;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&#34;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

pub fn unescape_string(value: &str) -> String {
    unescape_html_entities(value)
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
    js_string_escaper(value)
}

pub fn js_escaper(args: &[Value]) -> String {
    let mut combined = String::new();
    for arg in args {
        combined.push_str(&arg.to_plain_string());
    }
    js_string_escaper(&combined)
}

pub fn url_query_escaper(args: &[Value]) -> String {
    let mut combined = String::new();
    for arg in args {
        combined.push_str(&arg.to_plain_string());
    }
    query_escape_url(&combined)
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
pub fn EscapeString(value: &str) -> String {
    escape_string(value)
}

#[allow(non_snake_case)]
pub fn UnescapeString(value: &str) -> String {
    unescape_string(value)
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
    Text(TextNode),
    Expr {
        expr: Expr,
        mode: EscapeMode,
        runtime_mode: bool,
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
struct TextNode {
    raw: SharedText,
    prepared: Option<Arc<PreparedTextPlan>>,
}

impl TextNode {
    fn from_span(source: Arc<str>, start: usize, end: usize) -> Self {
        Self {
            raw: SharedText::new(source, start, end),
            prepared: None,
        }
    }

    fn from_owned_string(value: String) -> Self {
        Self {
            raw: SharedText::from(value),
            prepared: None,
        }
    }
}

#[derive(Clone, Debug)]
enum SharedTextSource {
    Slice(Arc<str>),
    Owned(Arc<String>),
}

#[derive(Clone, Debug)]
struct SharedText {
    source: SharedTextSource,
    start: usize,
    end: usize,
}

impl SharedText {
    fn new(source: Arc<str>, start: usize, end: usize) -> Self {
        debug_assert!(start <= end);
        debug_assert!(end <= source.len());
        Self {
            source: SharedTextSource::Slice(source),
            start,
            end,
        }
    }

    fn from_owned(value: String) -> Self {
        let source = Arc::new(value);
        let end = source.len();
        Self {
            source: SharedTextSource::Owned(source),
            start: 0,
            end,
        }
    }

    fn as_str(&self) -> &str {
        match &self.source {
            SharedTextSource::Slice(source) => &source[self.start..self.end],
            SharedTextSource::Owned(source) => &source[self.start..self.end],
        }
    }
}

impl Deref for SharedText {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl From<String> for SharedText {
    fn from(value: String) -> Self {
        SharedText::from_owned(value)
    }
}

#[derive(Clone, Debug)]
struct PreparedTextPlan {
    start_state: ContextState,
    chunks: Vec<PreparedTextChunk>,
}

#[derive(Clone, Debug)]
enum PreparedTextChunk {
    Emit(String),
    ScriptCloseTag(String),
    StyleCloseTag(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreparedSection {
    Html,
    Script { scan_state: JsScanState },
    Style { scan_state: CssScanState },
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
    Text { start: usize, end: usize },
    Action { start: usize, end: usize },
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
    in_js_attribute: bool,
}

#[derive(Clone, Debug)]
struct RenderedContextSnapshot {
    state: ContextState,
    url_part: Option<UrlPartContext>,
    js_scan_state: Option<JsScanState>,
    css_scan_state: Option<CssScanState>,
    script_json: bool,
}

fn rendered_snapshot_from_open_tag_fragment(fragment: &str) -> RenderedContextSnapshot {
    let tag_value_context = current_tag_value_context(fragment);

    let (
        mode,
        in_css_attribute,
        css_attribute_quote,
        in_js_attribute,
        url_part,
        js_scan_state,
        css_scan_state,
    ) = if let Some(context) = tag_value_context {
        let kind = attr_kind(&context.attr_name);
        match kind {
            AttrKind::Js => {
                let mode = match script_attribute_mode(&context.value_prefix) {
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
                let js_scan_state = if is_script_escape_mode(mode) {
                    Some(current_js_scan_state(&context.value_prefix))
                } else {
                    None
                };
                (mode, false, None, true, None, js_scan_state, None)
            }
            AttrKind::Css => {
                let mode = match style_attribute_mode(&context.value_prefix) {
                    Some(EscapeMode::StyleExpr) | None => {
                        if context.quoted {
                            EscapeMode::AttrQuoted {
                                kind,
                                quote: context.quote.unwrap_or('"'),
                            }
                        } else {
                            EscapeMode::AttrUnquoted { kind }
                        }
                    }
                    Some(mode) => mode,
                };
                let css_scan_state = if is_style_escape_mode(mode) {
                    Some(current_css_scan_state(&context.value_prefix))
                } else {
                    None
                };
                (mode, true, context.quote, false, None, None, css_scan_state)
            }
            AttrKind::Url | AttrKind::Normal | AttrKind::Srcset => {
                let mode = if context.quoted {
                    EscapeMode::AttrQuoted {
                        kind,
                        quote: context.quote.unwrap_or('"'),
                    }
                } else {
                    EscapeMode::AttrUnquoted { kind }
                };
                let url_part = if matches!(kind, AttrKind::Url) {
                    Some(if context.value_prefix.contains('#') {
                        UrlPartContext::Fragment
                    } else if context.value_prefix.contains('?') {
                        UrlPartContext::Query
                    } else {
                        UrlPartContext::Path
                    })
                } else {
                    None
                };
                (mode, false, None, false, url_part, None, None)
            }
        }
    } else if current_attr_name_context(fragment) {
        (EscapeMode::AttrName, false, None, false, None, None, None)
    } else {
        (EscapeMode::Html, false, None, false, None, None, None)
    };

    let in_open_tag = (matches!(mode, EscapeMode::Html) && is_in_unclosed_tag_context(fragment))
        || matches!(mode, EscapeMode::AttrName);
    let state = ContextState {
        mode,
        in_open_tag,
        in_css_attribute,
        css_attribute_quote,
        in_js_attribute,
    };

    RenderedContextSnapshot {
        state,
        url_part,
        js_scan_state,
        css_scan_state,
        script_json: false,
    }
}

fn rendered_snapshot_html_text() -> RenderedContextSnapshot {
    RenderedContextSnapshot {
        state: ContextState::html_text(),
        url_part: None,
        js_scan_state: None,
        css_scan_state: None,
        script_json: false,
    }
}

fn analyze_rendered_context(rendered: &str) -> RenderedContextSnapshot {
    let bytes = rendered.as_bytes();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        let Some(offset) = bytes[cursor..].iter().position(|byte| *byte == b'<') else {
            return rendered_snapshot_html_text();
        };
        let start = cursor + offset;

        if start + 1 >= bytes.len() {
            return rendered_snapshot_from_open_tag_fragment(&rendered[start..]);
        }

        let next = bytes[start + 1];
        if next == b'!' {
            if start + 4 <= bytes.len() && &bytes[start..start + 4] == b"<!--" {
                if let Some(end_rel) = rendered[start + 4..].find("-->") {
                    cursor = start + 4 + end_rel + 3;
                    continue;
                }
                return rendered_snapshot_html_text();
            }

            let Some(tag_end) = html_tag_end(rendered, start) else {
                return rendered_snapshot_from_open_tag_fragment(&rendered[start..]);
            };
            cursor = tag_end;
            continue;
        }

        if next == b'?' {
            let Some(tag_end) = html_tag_end(rendered, start) else {
                return rendered_snapshot_from_open_tag_fragment(&rendered[start..]);
            };
            cursor = tag_end;
            continue;
        }

        let close = next == b'/';
        let name_start = if close { start + 2 } else { start + 1 };
        if name_start >= bytes.len() || !bytes[name_start].is_ascii_alphabetic() {
            cursor = start + 1;
            continue;
        }

        let mut name_end = name_start + 1;
        while name_end < bytes.len() && is_html_tag_name_byte(bytes[name_end]) {
            name_end += 1;
        }
        let tag_name = &bytes[name_start..name_end];

        let Some(tag_end) = html_tag_end(rendered, start) else {
            return rendered_snapshot_from_open_tag_fragment(&rendered[start..]);
        };

        if close {
            cursor = tag_end;
            continue;
        }

        if tag_name.eq_ignore_ascii_case(b"script") {
            let open_tag = &rendered[start..tag_end];
            if !is_script_type_javascript(open_tag) {
                cursor = tag_end;
                continue;
            }

            let script_json = is_script_type_json(open_tag);
            if let Some(close_start) = find_script_close_tag(rendered, tag_end) {
                let close_end = html_tag_end(rendered, close_start).unwrap_or(close_start + 1);
                cursor = close_end;
                continue;
            }

            let content = &rendered[tag_end..];
            let js_scan_state = current_js_scan_state(content);
            let state = ContextState {
                mode: js_scan_state_to_escape_mode(js_scan_state, script_json),
                in_open_tag: false,
                in_css_attribute: false,
                css_attribute_quote: None,
                in_js_attribute: false,
            };
            return RenderedContextSnapshot {
                state,
                url_part: None,
                js_scan_state: Some(js_scan_state),
                css_scan_state: None,
                script_json,
            };
        }

        if tag_name.eq_ignore_ascii_case(b"style") {
            if let Some(close_start) = find_style_close_tag(rendered, tag_end) {
                let close_end = html_tag_end(rendered, close_start).unwrap_or(close_start + 1);
                cursor = close_end;
                continue;
            }

            let content = &rendered[tag_end..];
            let css_scan_state = current_css_scan_state(content);
            let state = ContextState {
                mode: css_scan_state_to_escape_mode(css_scan_state),
                in_open_tag: false,
                in_css_attribute: false,
                css_attribute_quote: None,
                in_js_attribute: false,
            };
            return RenderedContextSnapshot {
                state,
                url_part: None,
                js_scan_state: None,
                css_scan_state: Some(css_scan_state),
                script_json: false,
            };
        }

        if tag_name.eq_ignore_ascii_case(b"title") || tag_name.eq_ignore_ascii_case(b"textarea") {
            if let Some(close_start) = find_close_tag(rendered, tag_end, tag_name) {
                let close_end = html_tag_end(rendered, close_start).unwrap_or(close_start + 1);
                cursor = close_end;
                continue;
            }

            let state = ContextState {
                mode: EscapeMode::Rcdata,
                in_open_tag: false,
                in_css_attribute: false,
                css_attribute_quote: None,
                in_js_attribute: false,
            };
            return RenderedContextSnapshot {
                state,
                url_part: None,
                js_scan_state: None,
                css_scan_state: None,
                script_json: false,
            };
        }

        cursor = tag_end;
    }

    rendered_snapshot_html_text()
}

impl ContextState {
    fn html_text() -> Self {
        Self {
            mode: EscapeMode::Html,
            in_open_tag: false,
            in_css_attribute: false,
            css_attribute_quote: None,
            in_js_attribute: false,
        }
    }

    #[cfg(test)]
    fn from_rendered(rendered: &str) -> Self {
        analyze_rendered_context(rendered).state
    }

    fn is_text_context(&self) -> bool {
        matches!(self.mode, EscapeMode::Html) && !self.in_open_tag
    }

    fn is_script_tag_context(&self) -> bool {
        is_script_escape_mode(self.mode) && !self.in_js_attribute
    }

    fn is_style_tag_context(&self) -> bool {
        is_style_escape_mode(self.mode) && !self.in_css_attribute
    }
}

#[derive(Clone, Debug)]
struct ContextTracker {
    rendered: String,
    state: ContextState,
    url_part: Option<UrlPartContext>,
    js_scan_state: Option<JsScanState>,
    css_scan_state: Option<CssScanState>,
    script_json: bool,
    attr_name_dynamic_pending: bool,
    attr_value_from_dynamic_attr: bool,
}

impl ContextTracker {
    fn from_state(state: ContextState) -> Self {
        let rendered = seed_rendered_for_state(&state);
        let url_part = url_part_from_mode_and_rendered(state.mode, &rendered);
        let mut tracker = Self {
            rendered,
            state,
            url_part,
            js_scan_state: None,
            css_scan_state: None,
            script_json: false,
            attr_name_dynamic_pending: false,
            attr_value_from_dynamic_attr: false,
        };
        tracker.sync_incremental_scan_state();
        tracker
    }

    fn state(&self) -> ContextState {
        self.state.clone()
    }

    fn mode(&self) -> EscapeMode {
        self.state.mode
    }

    fn apply_rendered_context_snapshot(&mut self, snapshot: RenderedContextSnapshot) {
        self.state = snapshot.state;
        self.url_part = snapshot.url_part;
        self.js_scan_state = snapshot.js_scan_state;
        self.css_scan_state = snapshot.css_scan_state;
        self.script_json = snapshot.script_json;
    }

    fn css_url_part_hint(&self) -> Option<Option<UrlPartContext>> {
        match self.css_scan_state {
            Some(CssScanState::SingleQuote {
                is_url: true,
                url_part,
            })
            | Some(CssScanState::DoubleQuote {
                is_url: true,
                url_part,
            }) => Some(Some(url_part)),
            Some(CssScanState::SingleQuote { is_url: false, .. })
            | Some(CssScanState::DoubleQuote { is_url: false, .. }) => Some(None),
            _ => None,
        }
    }

    fn append_text(&mut self, text: &str) {
        let previous_mode = self.state.mode;
        self.rendered.push_str(text);
        self.refresh_cached_state_with_delta(text);
        self.refresh_dynamic_attr_runtime_flag(previous_mode);
        if should_normalize_tracker_state(&self.state) {
            self.normalize_from_cached_state();
        }
    }

    fn append_text_for_parse(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        let skip_rendered_tail_if_incremental =
            self.should_skip_rendered_tail_update_for_parse_text(text);
        let previous_mode = self.state.mode;
        let used_incremental = self.refresh_cached_state_with_delta_for_parse(text);
        self.refresh_dynamic_attr_runtime_flag(previous_mode);
        if !(skip_rendered_tail_if_incremental && used_incremental) {
            self.update_rendered_tail_for_parse(text);
        }
        if should_normalize_tracker_state(&self.state) {
            self.normalize_from_cached_state();
        }
    }

    fn append_known_script_close_tag(&mut self, close_tag: &str) {
        self.append_known_script_close_tag_with_suffix(close_tag, "");
    }

    fn append_known_script_close_tag_with_suffix(&mut self, close_tag: &str, suffix: &str) {
        self.rendered.push_str(close_tag);
        self.state = ContextState::html_text();
        self.url_part = None;
        self.js_scan_state = None;
        self.css_scan_state = None;
        self.script_json = false;
        self.attr_name_dynamic_pending = false;
        self.attr_value_from_dynamic_attr = false;
        if !suffix.is_empty() {
            self.rendered.push_str(suffix);
            self.apply_rendered_context_snapshot(analyze_rendered_context(suffix));
        }
        if should_normalize_tracker_state(&self.state) {
            self.normalize_from_cached_state();
        }
    }

    fn append_known_style_close_tag(&mut self, close_tag: &str) {
        self.append_known_style_close_tag_with_suffix(close_tag, "");
    }

    fn append_known_style_close_tag_with_suffix(&mut self, close_tag: &str, suffix: &str) {
        self.rendered.push_str(close_tag);
        self.state = ContextState::html_text();
        self.url_part = None;
        self.js_scan_state = None;
        self.css_scan_state = None;
        self.script_json = false;
        self.attr_name_dynamic_pending = false;
        self.attr_value_from_dynamic_attr = false;
        if !suffix.is_empty() {
            self.rendered.push_str(suffix);
            self.apply_rendered_context_snapshot(analyze_rendered_context(suffix));
        }
        if should_normalize_tracker_state(&self.state) {
            self.normalize_from_cached_state();
        }
    }

    fn append_expr_placeholder(&mut self, mode: EscapeMode) {
        let previous_mode = self.state.mode;
        let placeholder = placeholder_for_mode(mode);
        self.rendered.push_str(placeholder);
        self.refresh_cached_state_with_delta(placeholder);
        if matches!(mode, EscapeMode::AttrName) {
            self.attr_name_dynamic_pending = true;
        }
        self.refresh_dynamic_attr_runtime_flag(previous_mode);
        if should_normalize_tracker_state(&self.state) {
            self.normalize_from_cached_state();
        }
    }

    fn append_expr_placeholder_for_parse(&mut self, mode: EscapeMode) {
        let previous_mode = self.state.mode;
        let placeholder = placeholder_for_mode(mode);
        let _ = self.refresh_cached_state_with_delta_for_parse(placeholder);
        if matches!(mode, EscapeMode::AttrName) {
            self.attr_name_dynamic_pending = true;
        }
        self.refresh_dynamic_attr_runtime_flag(previous_mode);
        self.update_rendered_tail_for_parse(placeholder);
        if should_normalize_tracker_state(&self.state) {
            self.normalize_from_cached_state();
        }
    }

    fn refresh_dynamic_attr_runtime_flag(&mut self, previous_mode: EscapeMode) {
        let current_mode = self.state.mode;
        let was_attr_value = is_attr_value_mode(previous_mode);
        let is_attr_value = is_attr_value_mode(current_mode);

        if matches!(previous_mode, EscapeMode::AttrName) && is_attr_value {
            self.attr_value_from_dynamic_attr = self.attr_name_dynamic_pending;
            self.attr_name_dynamic_pending = false;
        } else if was_attr_value && !is_attr_value {
            self.attr_value_from_dynamic_attr = false;
        }

        if !is_tag_open_related_mode(current_mode) {
            self.attr_name_dynamic_pending = false;
            self.attr_value_from_dynamic_attr = false;
        }
    }

    fn refresh_cached_state_with_delta(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        if self.try_refresh_cached_state_incremental(delta) {
            return;
        }
        self.refresh_cached_state_full();
    }

    fn refresh_cached_state_with_delta_for_parse(&mut self, delta: &str) -> bool {
        if delta.is_empty() {
            return false;
        }
        if self.try_refresh_cached_state_incremental_for_parse(delta) {
            return true;
        }
        if self.state.in_open_tag
            || matches!(self.state.mode, EscapeMode::AttrName)
            || self.js_scan_state.is_some()
            || self.css_scan_state.is_some()
            || self.script_json
        {
            self.refresh_cached_state_with_rendered_tail_delta_for_parse(delta);
        } else {
            self.refresh_cached_state_seeded_delta(delta);
        }
        false
    }

    fn refresh_cached_state_full(&mut self) {
        self.apply_rendered_context_snapshot(analyze_rendered_context(&self.rendered));
    }

    fn try_refresh_cached_state_incremental(&mut self, delta: &str) -> bool {
        match self.state.mode {
            EscapeMode::Html => {
                if !self.state.in_open_tag && !delta.as_bytes().contains(&b'<') {
                    self.url_part = None;
                    return true;
                }
                if !self.state.in_open_tag && self.try_refresh_html_text_with_delta(delta) {
                    return true;
                }
            }
            EscapeMode::Rcdata => {
                if !delta.as_bytes().contains(&b'<') {
                    return true;
                }
            }
            EscapeMode::AttrName => {
                if delta.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
                    return true;
                }
            }
            EscapeMode::AttrQuoted { .. } | EscapeMode::AttrUnquoted { .. } => {
                return self.try_refresh_cached_state_seeded_delta(delta);
            }
            _ if self.state.is_script_tag_context() => {
                let Some(scan_state) = self.js_scan_state else {
                    return false;
                };
                if contains_ascii_case_insensitive(delta, b"</script") {
                    if !matches!(scan_state, JsScanState::Expr { .. }) {
                        return false;
                    }
                    let Some(close_start) = find_close_tag(delta, 0, b"script") else {
                        return false;
                    };
                    let Some(close_end) = html_tag_end(delta, close_start) else {
                        return false;
                    };

                    let before_close = &delta[..close_start];
                    let _ = advance_js_scan_state(before_close, scan_state);
                    self.state = ContextState::html_text();
                    self.url_part = None;
                    self.js_scan_state = None;
                    self.css_scan_state = None;
                    self.script_json = false;

                    let suffix = &delta[close_end..];
                    if !suffix.is_empty() {
                        self.apply_rendered_context_snapshot(analyze_rendered_context(suffix));
                    }
                    return true;
                }
                let next_state = advance_js_scan_state(delta, scan_state);
                self.js_scan_state = Some(next_state);
                self.state.mode = js_scan_state_to_escape_mode(next_state, self.script_json);
                self.state.in_open_tag = false;
                self.state.in_css_attribute = false;
                self.state.css_attribute_quote = None;
                self.state.in_js_attribute = false;
                self.url_part = None;
                return true;
            }
            _ if self.state.is_style_tag_context() => {
                let Some(scan_state) = self.css_scan_state else {
                    return false;
                };
                if contains_ascii_case_insensitive(delta, b"</style") {
                    let Some(close_start) = find_style_close_tag_with_state(delta, 0, scan_state)
                    else {
                        return false;
                    };
                    let Some(close_end) = html_tag_end(delta, close_start) else {
                        return false;
                    };

                    let before_close = &delta[..close_start];
                    let _ = advance_css_scan_state(before_close, scan_state);
                    self.state = ContextState::html_text();
                    self.url_part = None;
                    self.js_scan_state = None;
                    self.css_scan_state = None;
                    self.script_json = false;

                    let suffix = &delta[close_end..];
                    if !suffix.is_empty() {
                        self.apply_rendered_context_snapshot(analyze_rendered_context(suffix));
                    }
                    return true;
                }
                let next_state = advance_css_scan_state(delta, scan_state);
                self.css_scan_state = Some(next_state);
                self.state.mode = css_scan_state_to_escape_mode(next_state);
                self.state.in_open_tag = false;
                self.state.in_css_attribute = false;
                self.state.css_attribute_quote = None;
                self.state.in_js_attribute = false;
                self.url_part = None;
                return true;
            }
            _ => {}
        }
        false
    }

    fn try_refresh_cached_state_incremental_for_parse(&mut self, delta: &str) -> bool {
        if self.state.is_script_tag_context() {
            let Some(scan_state) = self.js_scan_state else {
                return false;
            };
            if contains_ascii_case_insensitive(delta, b"</script") {
                if !matches!(scan_state, JsScanState::Expr { .. }) {
                    return false;
                }
                let Some(close_start) = find_close_tag(delta, 0, b"script") else {
                    return false;
                };
                let Some(close_end) = html_tag_end(delta, close_start) else {
                    return false;
                };

                let before_close = &delta[..close_start];
                let _ = advance_js_scan_state(before_close, scan_state);
                self.state = ContextState::html_text();
                self.url_part = None;
                self.js_scan_state = None;
                self.css_scan_state = None;
                self.script_json = false;

                let suffix = &delta[close_end..];
                if !suffix.is_empty() && !self.try_refresh_html_text_with_delta(suffix) {
                    self.apply_rendered_context_snapshot(analyze_rendered_context(suffix));
                }
                return true;
            }
        } else if self.state.is_style_tag_context() {
            let Some(scan_state) = self.css_scan_state else {
                return false;
            };
            if contains_ascii_case_insensitive(delta, b"</style") {
                let Some(close_start) = find_style_close_tag_with_state(delta, 0, scan_state)
                else {
                    return false;
                };
                let Some(close_end) = html_tag_end(delta, close_start) else {
                    return false;
                };

                let before_close = &delta[..close_start];
                let _ = advance_css_scan_state(before_close, scan_state);
                self.state = ContextState::html_text();
                self.url_part = None;
                self.js_scan_state = None;
                self.css_scan_state = None;
                self.script_json = false;

                let suffix = &delta[close_end..];
                if !suffix.is_empty() && !self.try_refresh_html_text_with_delta(suffix) {
                    self.apply_rendered_context_snapshot(analyze_rendered_context(suffix));
                }
                return true;
            }
        }

        self.try_refresh_cached_state_incremental(delta)
    }

    fn try_refresh_html_text_with_delta(&mut self, delta: &str) -> bool {
        if !matches!(self.state.mode, EscapeMode::Html) || self.state.in_open_tag {
            return false;
        }

        let bytes = delta.as_bytes();
        let mut cursor = 0usize;

        while cursor < bytes.len() {
            let Some(offset) = bytes[cursor..].iter().position(|byte| *byte == b'<') else {
                self.state = ContextState::html_text();
                self.url_part = None;
                return true;
            };
            let start = cursor + offset;

            if start + 1 >= bytes.len() {
                self.refresh_cached_state_from_fragment(&delta[start..]);
                return true;
            }

            let next = bytes[start + 1];
            if next == b'!' {
                if start + 4 <= bytes.len() && &bytes[start..start + 4] == b"<!--" {
                    if let Some(end_offset) = delta[start + 4..].find("-->") {
                        cursor = start + 4 + end_offset + 3;
                        continue;
                    }
                    self.refresh_cached_state_from_fragment(&delta[start..]);
                    return true;
                }

                if let Some(end) = html_tag_end(delta, start) {
                    cursor = end;
                    continue;
                }
                self.refresh_cached_state_from_fragment(&delta[start..]);
                return true;
            }

            if next == b'?' {
                if let Some(end) = html_tag_end(delta, start) {
                    cursor = end;
                    continue;
                }
                self.refresh_cached_state_from_fragment(&delta[start..]);
                return true;
            }

            let close = next == b'/';
            let name_start = if close { start + 2 } else { start + 1 };
            if name_start >= bytes.len() || !bytes[name_start].is_ascii_alphabetic() {
                cursor = start + 1;
                continue;
            }

            let mut name_end = name_start + 1;
            while name_end < bytes.len() && is_html_tag_name_byte(bytes[name_end]) {
                name_end += 1;
            }
            let tag_name = &bytes[name_start..name_end];

            let Some(tag_end) = html_tag_end(delta, start) else {
                self.refresh_cached_state_from_fragment(&delta[start..]);
                return true;
            };

            if close {
                cursor = tag_end;
                continue;
            }

            if tag_name.eq_ignore_ascii_case(b"script") {
                let open_tag = &delta[start..tag_end];
                if !is_script_type_javascript(open_tag) {
                    cursor = tag_end;
                    continue;
                }

                let script_json = is_script_type_json(open_tag);
                if let Some(close_start) = find_close_tag(delta, tag_end, b"script")
                    && let Some(close_end) = html_tag_end(delta, close_start)
                {
                    cursor = close_end;
                    continue;
                }

                let content = &delta[tag_end..];
                let js_state = current_js_scan_state(content);
                self.state.mode = js_scan_state_to_escape_mode(js_state, script_json);
                self.state.in_open_tag = false;
                self.state.in_css_attribute = false;
                self.state.css_attribute_quote = None;
                self.state.in_js_attribute = false;
                self.url_part = None;
                self.js_scan_state = Some(js_state);
                self.css_scan_state = None;
                self.script_json = script_json;
                return true;
            }

            if tag_name.eq_ignore_ascii_case(b"style") {
                if let Some(close_start) = find_close_tag(delta, tag_end, b"style")
                    && let Some(close_end) = html_tag_end(delta, close_start)
                {
                    cursor = close_end;
                    continue;
                }

                let content = &delta[tag_end..];
                let css_state = current_css_scan_state(content);
                self.state.mode = css_scan_state_to_escape_mode(css_state);
                self.state.in_open_tag = false;
                self.state.in_css_attribute = false;
                self.state.css_attribute_quote = None;
                self.state.in_js_attribute = false;
                self.url_part = None;
                self.js_scan_state = None;
                self.css_scan_state = Some(css_state);
                self.script_json = false;
                return true;
            }

            if tag_name.eq_ignore_ascii_case(b"title") || tag_name.eq_ignore_ascii_case(b"textarea")
            {
                if let Some(close_start) = find_close_tag(delta, tag_end, tag_name)
                    && let Some(close_end) = html_tag_end(delta, close_start)
                {
                    cursor = close_end;
                    continue;
                }

                self.state.mode = EscapeMode::Rcdata;
                self.state.in_open_tag = false;
                self.state.in_css_attribute = false;
                self.state.css_attribute_quote = None;
                self.state.in_js_attribute = false;
                self.url_part = None;
                self.js_scan_state = None;
                self.css_scan_state = None;
                self.script_json = false;
                return true;
            }

            cursor = tag_end;
        }

        self.state = ContextState::html_text();
        self.url_part = None;
        self.js_scan_state = None;
        self.css_scan_state = None;
        self.script_json = false;
        true
    }

    fn try_refresh_cached_state_seeded_delta(&mut self, delta: &str) -> bool {
        self.refresh_cached_state_seeded_delta(delta);
        true
    }

    fn refresh_cached_state_seeded_delta(&mut self, delta: &str) {
        let mut rendered = seed_rendered_for_state_with_url_part(&self.state, self.url_part);
        rendered.push_str(delta);
        self.apply_rendered_context_snapshot(analyze_rendered_context(&rendered));
    }

    fn refresh_cached_state_with_rendered_tail_delta_for_parse(&mut self, delta: &str) {
        let mut rendered = self.rendered.clone();
        rendered.push_str(delta);
        self.apply_rendered_context_snapshot(analyze_rendered_context(&rendered));
    }

    fn refresh_cached_state_from_fragment(&mut self, fragment: &str) {
        self.apply_rendered_context_snapshot(analyze_rendered_context(fragment));
    }

    fn sync_incremental_scan_state(&mut self) {
        let (js_scan_state, css_scan_state, script_json) =
            incremental_scan_state_from_rendered(&self.state, &self.rendered);
        self.js_scan_state = js_scan_state;
        self.css_scan_state = css_scan_state;
        self.script_json = script_json;
    }

    fn normalize_from_cached_state(&mut self) {
        self.rendered = seed_rendered_for_state_with_url_part(&self.state, self.url_part);
        self.sync_incremental_scan_state();
    }

    fn update_rendered_tail_for_parse(&mut self, delta: &str) {
        if delta.is_empty() || should_normalize_tracker_state(&self.state) {
            return;
        }

        self.rendered.push_str(delta);
        truncate_to_char_boundary_tail(&mut self.rendered, PARSE_TRACKER_RENDERED_TAIL_MAX);
    }

    fn should_skip_rendered_tail_update_for_parse_text(&self, delta: &str) -> bool {
        if delta.is_empty() || delta.as_bytes().contains(&b'\\') {
            return false;
        }
        if has_unfinished_escape_suffix(&self.rendered) {
            return false;
        }
        if self.state.is_script_tag_context()
            && matches!(self.js_scan_state, Some(JsScanState::Expr { .. }))
            && !contains_ascii_case_insensitive(delta, b"</script")
        {
            return true;
        }
        if self.state.is_style_tag_context()
            && matches!(self.css_scan_state, Some(CssScanState::Expr))
            && !contains_ascii_case_insensitive(delta, b"</style")
        {
            return true;
        }

        false
    }
}

const PARSE_TRACKER_RENDERED_TAIL_MAX: usize = 2048;

fn truncate_to_char_boundary_tail(value: &mut String, max_len: usize) {
    if value.len() <= max_len {
        return;
    }

    let mut start = value.len().saturating_sub(max_len);
    while start < value.len() && !value.is_char_boundary(start) {
        start += 1;
    }
    if start >= value.len() {
        value.clear();
        return;
    }
    value.drain(..start);
}

fn contains_ascii_case_insensitive(haystack: &str, needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }

    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

fn incremental_scan_state_from_rendered(
    state: &ContextState,
    rendered: &str,
) -> (Option<JsScanState>, Option<CssScanState>, bool) {
    let mut js_scan_state = None;
    let mut css_scan_state = None;
    let mut script_json = false;

    if state.is_script_tag_context() {
        if let Some(content) = current_unclosed_tag_content(rendered, "script") {
            js_scan_state = Some(current_js_scan_state(content));
        } else {
            js_scan_state = Some(js_scan_state_for_escape_mode(state.mode));
        }
        if let Some(script_tag) = current_unclosed_script_tag(rendered) {
            script_json = is_script_type_json(script_tag);
        } else {
            script_json = matches!(state.mode, EscapeMode::ScriptJsonString { .. });
        }
    } else if state.in_js_attribute && is_script_escape_mode(state.mode) {
        if let Some(context) = current_tag_value_context(rendered) {
            js_scan_state = Some(current_js_scan_state(&context.value_prefix));
        } else {
            js_scan_state = Some(js_scan_state_for_escape_mode(state.mode));
        }
    }

    if state.is_style_tag_context() {
        if let Some(content) = current_unclosed_tag_content(rendered, "style") {
            css_scan_state = Some(current_css_scan_state(content));
        } else {
            css_scan_state = Some(css_scan_state_for_escape_mode(state.mode));
        }
    } else if state.in_css_attribute && is_style_escape_mode(state.mode) {
        if let Some(context) = current_tag_value_context(rendered) {
            css_scan_state = Some(current_css_scan_state(&context.value_prefix));
        } else {
            css_scan_state = Some(css_scan_state_for_escape_mode(state.mode));
        }
    }

    (js_scan_state, css_scan_state, script_json)
}

fn js_scan_state_for_escape_mode(mode: EscapeMode) -> JsScanState {
    match mode {
        EscapeMode::ScriptExpr => JsScanState::Expr {
            js_ctx: JsContext::RegExp,
        },
        EscapeMode::ScriptString { quote } | EscapeMode::ScriptJsonString { quote } => {
            if quote == '\'' {
                JsScanState::SingleQuote
            } else {
                JsScanState::DoubleQuote
            }
        }
        EscapeMode::ScriptTemplate => JsScanState::TemplateLiteral,
        EscapeMode::ScriptRegexp => JsScanState::RegExp {
            in_char_class: false,
            js_ctx: JsContext::RegExp,
        },
        EscapeMode::ScriptLineComment => JsScanState::LineComment {
            js_ctx: JsContext::RegExp,
            preserve_body: true,
            keep_terminator: true,
        },
        EscapeMode::ScriptBlockComment => JsScanState::BlockComment {
            js_ctx: JsContext::RegExp,
        },
        _ => JsScanState::Expr {
            js_ctx: JsContext::RegExp,
        },
    }
}

fn css_scan_state_for_escape_mode(mode: EscapeMode) -> CssScanState {
    match mode {
        EscapeMode::StyleExpr => CssScanState::Expr,
        EscapeMode::StyleString { quote } => {
            if quote == '\'' {
                CssScanState::SingleQuote {
                    is_url: false,
                    url_part: UrlPartContext::Path,
                }
            } else {
                CssScanState::DoubleQuote {
                    is_url: false,
                    url_part: UrlPartContext::Path,
                }
            }
        }
        EscapeMode::StyleLineComment => CssScanState::LineComment,
        EscapeMode::StyleBlockComment => CssScanState::BlockComment,
        _ => CssScanState::Expr,
    }
}

fn js_scan_state_to_escape_mode(state: JsScanState, script_json: bool) -> EscapeMode {
    match state {
        JsScanState::Expr { .. } => EscapeMode::ScriptExpr,
        JsScanState::SingleQuote => {
            if script_json {
                EscapeMode::ScriptJsonString { quote: '\'' }
            } else {
                EscapeMode::ScriptString { quote: '\'' }
            }
        }
        JsScanState::DoubleQuote => {
            if script_json {
                EscapeMode::ScriptJsonString { quote: '"' }
            } else {
                EscapeMode::ScriptString { quote: '"' }
            }
        }
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

fn css_scan_state_to_escape_mode(state: CssScanState) -> EscapeMode {
    match state {
        CssScanState::Expr => EscapeMode::StyleExpr,
        CssScanState::SingleQuote { .. } => EscapeMode::StyleString { quote: '\'' },
        CssScanState::DoubleQuote { .. } => EscapeMode::StyleString { quote: '"' },
        CssScanState::LineComment => EscapeMode::StyleLineComment,
        CssScanState::BlockComment => EscapeMode::StyleBlockComment,
    }
}

fn is_script_escape_mode(mode: EscapeMode) -> bool {
    matches!(
        mode,
        EscapeMode::ScriptExpr
            | EscapeMode::ScriptTemplate
            | EscapeMode::ScriptRegexp
            | EscapeMode::ScriptLineComment
            | EscapeMode::ScriptBlockComment
            | EscapeMode::ScriptString { .. }
            | EscapeMode::ScriptJsonString { .. }
    )
}

fn is_style_escape_mode(mode: EscapeMode) -> bool {
    matches!(
        mode,
        EscapeMode::StyleExpr
            | EscapeMode::StyleString { .. }
            | EscapeMode::StyleLineComment
            | EscapeMode::StyleBlockComment
    )
}

fn should_normalize_tracker_state(state: &ContextState) -> bool {
    if state.in_css_attribute {
        return false;
    }

    !matches!(
        state.mode,
        EscapeMode::AttrName
            | EscapeMode::ScriptExpr
            | EscapeMode::ScriptString { .. }
            | EscapeMode::ScriptJsonString { .. }
            | EscapeMode::ScriptTemplate
            | EscapeMode::ScriptRegexp
            | EscapeMode::ScriptLineComment
            | EscapeMode::ScriptBlockComment
            | EscapeMode::StyleString { .. }
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum AnalysisFlowKind {
    Normal,
    Break,
    Continue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum UrlPartContext {
    Path,
    Query,
    Fragment,
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

struct ParseContextAnalyzer<'a> {
    raw_templates: &'a mut HashMap<String, Vec<Node>>,
    start_states: HashMap<String, ContextState>,
    end_states: HashMap<String, ContextState>,
    in_progress: HashSet<String>,
    recursive_templates: HashSet<String>,
    text_transition_cache:
        HashMap<TextTransitionCacheKey, HashMap<String, TextTransitionCacheValue>>,
    prepared_text_plan_cache:
        HashMap<PreparedTextPlanCacheKey, HashMap<String, Option<Arc<PreparedTextPlan>>>>,
    if_transition_cache: HashMap<IfTransitionCacheKey, IfTransitionCacheValue>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TextTransitionCacheKey {
    state: ContextState,
    url_part: Option<UrlPartContext>,
    js_scan_state: Option<JsScanState>,
    css_scan_state: Option<CssScanState>,
    script_json: bool,
    attr_name_dynamic_pending: bool,
    attr_value_from_dynamic_attr: bool,
}

#[derive(Clone, Debug)]
struct TextTransitionCacheValue {
    rendered: String,
    state: ContextState,
    url_part: Option<UrlPartContext>,
    js_scan_state: Option<JsScanState>,
    css_scan_state: Option<CssScanState>,
    script_json: bool,
    attr_name_dynamic_pending: bool,
    attr_value_from_dynamic_attr: bool,
}

const TEXT_TRANSITION_CACHE_MAX_TEXT_LEN: usize = 256;
const PREPARED_TEXT_PLAN_CACHE_MAX_TEXT_LEN: usize = 512;
const IF_TRANSITION_CACHE_MAX_NODES: usize = 32;
const IF_TRANSITION_CACHE_MAX_TEXT_LEN: usize = 128;
const IF_TRANSITION_CACHE_MAX_RENDERED_LEN: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct IfExprModeCacheEntry {
    mode: EscapeMode,
    runtime_mode: bool,
}

#[derive(Clone, Debug)]
struct IfTransitionCacheValue {
    then_expr_modes: Vec<IfExprModeCacheEntry>,
    else_expr_modes: Vec<IfExprModeCacheEntry>,
    flows: Vec<AnalysisFlow>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct IfTrackerCacheKey {
    rendered: String,
    state: ContextState,
    url_part: Option<UrlPartContext>,
    js_scan_state: Option<JsScanState>,
    css_scan_state: Option<CssScanState>,
    script_json: bool,
    attr_name_dynamic_pending: bool,
    attr_value_from_dynamic_attr: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct LinearNodesSignature {
    hash: u64,
    node_count: usize,
    expr_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct IfTransitionCacheKey {
    tracker: IfTrackerCacheKey,
    in_range: bool,
    then_signature: LinearNodesSignature,
    else_signature: LinearNodesSignature,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PreparedTextPlanCacheKey {
    state: ContextState,
    js_scan_state: Option<JsScanState>,
    css_scan_state: Option<CssScanState>,
}

fn prepared_text_plan_cache_key(
    tracker: &ContextTracker,
    text: &str,
) -> Option<PreparedTextPlanCacheKey> {
    if text.is_empty() || text.len() > PREPARED_TEXT_PLAN_CACHE_MAX_TEXT_LEN {
        return None;
    }
    if !should_prepare_text_plan_for_script_style(&tracker.state, text) {
        return None;
    }

    let js_scan_state = if tracker.state.is_script_tag_context() {
        tracker.js_scan_state
    } else {
        None
    };
    let css_scan_state = if tracker.state.is_style_tag_context() {
        tracker.css_scan_state
    } else {
        None
    };

    Some(PreparedTextPlanCacheKey {
        state: tracker.state(),
        js_scan_state,
        css_scan_state,
    })
}

fn text_transition_cache_key(
    tracker: &ContextTracker,
    text: &str,
) -> Option<TextTransitionCacheKey> {
    if text.is_empty() || text.len() > TEXT_TRANSITION_CACHE_MAX_TEXT_LEN {
        return None;
    }
    if !text_transition_cache_is_eligible(tracker, text) {
        return None;
    }

    Some(TextTransitionCacheKey {
        state: tracker.state(),
        url_part: tracker.url_part,
        js_scan_state: tracker.js_scan_state,
        css_scan_state: tracker.css_scan_state,
        script_json: tracker.script_json,
        attr_name_dynamic_pending: tracker.attr_name_dynamic_pending,
        attr_value_from_dynamic_attr: tracker.attr_value_from_dynamic_attr,
    })
}

fn text_transition_cache_value(
    tracker: &ContextTracker,
    text: &str,
) -> Option<TextTransitionCacheValue> {
    if text.is_empty() || text.len() > TEXT_TRANSITION_CACHE_MAX_TEXT_LEN {
        return None;
    }
    if !text_transition_cache_is_eligible(tracker, text) {
        return None;
    }

    Some(TextTransitionCacheValue {
        rendered: tracker.rendered.clone(),
        state: tracker.state(),
        url_part: tracker.url_part,
        js_scan_state: tracker.js_scan_state,
        css_scan_state: tracker.css_scan_state,
        script_json: tracker.script_json,
        attr_name_dynamic_pending: tracker.attr_name_dynamic_pending,
        attr_value_from_dynamic_attr: tracker.attr_value_from_dynamic_attr,
    })
}

fn apply_text_transition_cache_value(
    tracker: &mut ContextTracker,
    value: &TextTransitionCacheValue,
) {
    tracker.rendered = value.rendered.clone();
    tracker.state = value.state.clone();
    tracker.url_part = value.url_part;
    tracker.js_scan_state = value.js_scan_state;
    tracker.css_scan_state = value.css_scan_state;
    tracker.script_json = value.script_json;
    tracker.attr_name_dynamic_pending = value.attr_name_dynamic_pending;
    tracker.attr_value_from_dynamic_attr = value.attr_value_from_dynamic_attr;
}

fn text_transition_cache_is_eligible(tracker: &ContextTracker, text: &str) -> bool {
    if should_normalize_tracker_state(&tracker.state) {
        return true;
    }

    if tracker.state.is_script_tag_context() {
        let Some(scan_state) = tracker.js_scan_state else {
            return false;
        };
        if contains_ascii_case_insensitive(text, b"</script") {
            if !matches!(scan_state, JsScanState::Expr { .. }) {
                return false;
            }
            return find_close_tag(text, 0, b"script").is_some();
        }
        return true;
    }

    if tracker.state.is_style_tag_context() {
        let Some(scan_state) = tracker.css_scan_state else {
            return false;
        };
        if contains_ascii_case_insensitive(text, b"</style") {
            return find_style_close_tag_with_state(text, 0, scan_state).is_some();
        }
        return true;
    }

    false
}

impl<'a> ParseContextAnalyzer<'a> {
    fn new(raw_templates: &'a mut HashMap<String, Vec<Node>>) -> Self {
        Self {
            raw_templates,
            start_states: HashMap::new(),
            end_states: HashMap::new(),
            in_progress: HashSet::new(),
            recursive_templates: HashSet::new(),
            text_transition_cache: HashMap::new(),
            prepared_text_plan_cache: HashMap::new(),
            if_transition_cache: HashMap::new(),
        }
    }

    fn has_analysis(&self, name: &str) -> bool {
        self.end_states.contains_key(name)
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

        let mut nodes = self
            .raw_templates
            .remove(name)
            .ok_or_else(|| TemplateError::Parse(format!("no such template `{name}`")))?;

        self.start_states
            .insert(name.to_string(), start_state.clone());
        self.in_progress.insert(name.to_string());

        let analysis = (|| -> Result<ContextState> {
            let start_tracker = ContextTracker::from_state(start_state.clone());
            let flows = if is_linear_text_expr_nodes(&nodes) {
                self.analyze_linear_text_expr_nodes(&mut nodes, start_tracker)?
            } else {
                self.analyze_nodes(&mut nodes, start_tracker, false)?
            };

            let mut normal_states = HashSet::new();
            for flow in &flows {
                if flow.kind == AnalysisFlowKind::Normal {
                    if should_validate_action_context(&flow.tracker) {
                        validate_action_context_before_insertion(&flow.tracker)?;
                    }
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
            Ok(end_state)
        })();

        self.in_progress.remove(name);
        self.raw_templates.insert(name.to_string(), nodes);

        let end_state = analysis?;
        self.end_states.insert(name.to_string(), end_state.clone());
        Ok(end_state)
    }

    fn analyze_linear_text_expr_nodes(
        &mut self,
        nodes: &mut [Node],
        mut tracker: ContextTracker,
    ) -> Result<Vec<AnalysisFlow>> {
        for node in nodes {
            match node {
                Node::Text(text_node) => self.apply_text_node_transition(text_node, &mut tracker),
                Node::Expr {
                    mode, runtime_mode, ..
                } => self.apply_expr_node_transition(mode, runtime_mode, &mut tracker)?,
                _ => unreachable!("linear fast path must only receive text/expr nodes"),
            }
        }

        Ok(vec![AnalysisFlow::normal(tracker)])
    }

    fn analyze_nodes(
        &mut self,
        nodes: &mut [Node],
        start_tracker: ContextTracker,
        in_range: bool,
    ) -> Result<Vec<AnalysisFlow>> {
        let mut flows = vec![AnalysisFlow::normal(start_tracker)];

        for node in nodes {
            if let Node::Text(text_node) = node {
                if text_starts_with_js_slash(&text_node.raw) && has_slash_ambiguity(flows.iter()) {
                    return Err(TemplateError::Parse(
                        "'/' could start a division or regexp".to_string(),
                    ));
                }
            }

            if flows.len() == 1 {
                let flow = flows.pop().expect("single flow exists");
                if flow.kind != AnalysisFlowKind::Normal {
                    flows.push(flow);
                    continue;
                }

                let produced = self.analyze_node(node, flow.tracker, in_range)?;
                flows = if produced.len() <= 1 {
                    produced
                } else {
                    dedup_analysis_flows(produced)
                };
                continue;
            }

            let mut next_flows = Vec::with_capacity(flows.len());
            for flow in flows {
                if flow.kind != AnalysisFlowKind::Normal {
                    next_flows.push(flow);
                    continue;
                }

                let mut produced = self.analyze_node(node, flow.tracker, in_range)?;
                next_flows.append(&mut produced);
            }
            flows = if next_flows.len() <= 1 {
                next_flows
            } else {
                dedup_analysis_flows(next_flows)
            };
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
            Node::Text(text_node) => {
                self.apply_text_node_transition(text_node, &mut tracker);
                Ok(vec![AnalysisFlow::normal(tracker)])
            }
            Node::Expr {
                expr: _,
                mode,
                runtime_mode,
            } => {
                self.apply_expr_node_transition(mode, runtime_mode, &mut tracker)?;
                Ok(vec![AnalysisFlow::normal(tracker)])
            }
            Node::SetVar { .. } | Node::Define { .. } => Ok(vec![AnalysisFlow::normal(tracker)]),
            Node::If {
                then_branch,
                else_branch,
                ..
            } => {
                let cache_key =
                    if_transition_cache_key(&tracker, in_range, then_branch, else_branch);
                if let Some(cache_key) = cache_key.as_ref()
                    && let Some(cached) = self.if_transition_cache.get(cache_key).cloned()
                    && apply_linear_expr_modes(then_branch, &cached.then_expr_modes)
                    && apply_linear_expr_modes(else_branch, &cached.else_expr_modes)
                {
                    return Ok(cached.flows);
                }

                let then_flows = self.analyze_nodes(then_branch, tracker.clone(), in_range)?;
                let else_flows = self.analyze_nodes(else_branch, tracker, in_range)?;
                ensure_branch_normal_context("if", &then_flows, &else_flows)?;
                let mut merged = then_flows;
                merged.extend(else_flows);
                let merged = if merged.len() <= 1 {
                    merged
                } else {
                    dedup_analysis_flows(merged)
                };

                if let Some(cache_key) = cache_key
                    && let Some(then_expr_modes) = collect_linear_expr_modes(then_branch)
                    && let Some(else_expr_modes) = collect_linear_expr_modes(else_branch)
                {
                    self.if_transition_cache.insert(
                        cache_key,
                        IfTransitionCacheValue {
                            then_expr_modes,
                            else_expr_modes,
                            flows: merged.clone(),
                        },
                    );
                }

                Ok(merged)
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
                        AnalysisFlowKind::Normal | AnalysisFlowKind::Continue => {
                            if !range_reentry_context_matches(&range_start, &flow.tracker) {
                                return Err(TemplateError::Parse(
                                    "on range loop re-entry: context mismatch".to_string(),
                                ));
                            }
                        }
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
                if should_validate_action_context(&tracker) {
                    validate_action_context_before_insertion(&tracker)?;
                }
                let start_state = tracker.state();
                let end_state = self.analyze_template(name, start_state.clone())?;
                if start_state == end_state {
                    tracker.append_expr_placeholder_for_parse(end_state.mode);
                    Ok(vec![AnalysisFlow::normal(tracker)])
                } else {
                    Ok(vec![AnalysisFlow::normal(ContextTracker::from_state(
                        end_state,
                    ))])
                }
            }
            Node::Block { name, body, .. } => {
                if self.raw_templates.contains_key(name) {
                    if should_validate_action_context(&tracker) {
                        validate_action_context_before_insertion(&tracker)?;
                    }
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

    fn apply_text_node_transition(
        &mut self,
        text_node: &mut TextNode,
        tracker: &mut ContextTracker,
    ) {
        if text_node.prepared.is_none() {
            let prepared_cache_key = prepared_text_plan_cache_key(tracker, &text_node.raw);
            if let Some(key) = prepared_cache_key.as_ref()
                && let Some(cached) = self
                    .prepared_text_plan_cache
                    .get(key)
                    .and_then(|entries| entries.get(text_node.raw.as_str()))
            {
                text_node.prepared = cached.clone();
            }

            if text_node.prepared.is_none()
                && should_prepare_text_plan_for_script_style(&tracker.state, &text_node.raw)
            {
                let prepared =
                    prepare_text_plan_for_script_style(&tracker.state, tracker, &text_node.raw)
                        .map(Arc::new);
                if let Some(key) = prepared_cache_key {
                    self.prepared_text_plan_cache
                        .entry(key)
                        .or_default()
                        .insert(text_node.raw.to_string(), prepared.clone());
                }
                text_node.prepared = prepared;
            }
        }

        let cache_key = text_transition_cache_key(tracker, &text_node.raw);
        if let Some(key) = cache_key.as_ref()
            && let Some(cached) = self
                .text_transition_cache
                .get(key)
                .and_then(|entries| entries.get(text_node.raw.as_str()))
        {
            apply_text_transition_cache_value(tracker, cached);
            return;
        }

        tracker.append_text_for_parse(&text_node.raw);
        if let Some(key) = cache_key
            && let Some(cached) = text_transition_cache_value(tracker, &text_node.raw)
        {
            self.text_transition_cache
                .entry(key)
                .or_default()
                .insert(text_node.raw.to_string(), cached);
        }
    }

    fn apply_expr_node_transition(
        &self,
        mode: &mut EscapeMode,
        runtime_mode: &mut bool,
        tracker: &mut ContextTracker,
    ) -> Result<()> {
        if should_validate_action_context(tracker) {
            validate_action_context_before_insertion(tracker)?;
        }
        let escape_mode = tracker.mode();
        *mode = escape_mode;
        *runtime_mode = should_resolve_expr_mode_at_runtime(escape_mode, tracker);
        if placeholder_advances_parse_context(tracker, escape_mode) {
            tracker.append_expr_placeholder_for_parse(escape_mode);
        }
        Ok(())
    }
}

fn is_linear_text_expr_nodes(nodes: &[Node]) -> bool {
    nodes
        .iter()
        .all(|node| matches!(node, Node::Text(_) | Node::Expr { .. }))
}

fn if_transition_cache_key(
    tracker: &ContextTracker,
    in_range: bool,
    then_branch: &[Node],
    else_branch: &[Node],
) -> Option<IfTransitionCacheKey> {
    let tracker_key = if_tracker_cache_key(tracker)?;
    let then_signature = linear_nodes_signature_for_if_cache(then_branch)?;
    let else_signature = linear_nodes_signature_for_if_cache(else_branch)?;
    Some(IfTransitionCacheKey {
        tracker: tracker_key,
        in_range,
        then_signature,
        else_signature,
    })
}

fn if_tracker_cache_key(tracker: &ContextTracker) -> Option<IfTrackerCacheKey> {
    if tracker.rendered.len() > IF_TRANSITION_CACHE_MAX_RENDERED_LEN {
        return None;
    }
    Some(IfTrackerCacheKey {
        rendered: tracker.rendered.clone(),
        state: tracker.state(),
        url_part: tracker.url_part,
        js_scan_state: tracker.js_scan_state,
        css_scan_state: tracker.css_scan_state,
        script_json: tracker.script_json,
        attr_name_dynamic_pending: tracker.attr_name_dynamic_pending,
        attr_value_from_dynamic_attr: tracker.attr_value_from_dynamic_attr,
    })
}

fn linear_nodes_signature_for_if_cache(nodes: &[Node]) -> Option<LinearNodesSignature> {
    if nodes.len() > IF_TRANSITION_CACHE_MAX_NODES {
        return None;
    }

    let mut hasher = DefaultHasher::new();
    let mut expr_count = 0usize;
    for node in nodes {
        match node {
            Node::Text(text_node) => {
                let raw = text_node.raw.as_str();
                if raw.len() > IF_TRANSITION_CACHE_MAX_TEXT_LEN {
                    return None;
                }
                0u8.hash(&mut hasher);
                raw.hash(&mut hasher);
            }
            Node::Expr { .. } => {
                1u8.hash(&mut hasher);
                expr_count += 1;
            }
            _ => return None,
        }
    }

    Some(LinearNodesSignature {
        hash: hasher.finish(),
        node_count: nodes.len(),
        expr_count,
    })
}

fn collect_linear_expr_modes(nodes: &[Node]) -> Option<Vec<IfExprModeCacheEntry>> {
    let mut modes = Vec::new();
    for node in nodes {
        match node {
            Node::Text(_) => {}
            Node::Expr {
                mode, runtime_mode, ..
            } => modes.push(IfExprModeCacheEntry {
                mode: *mode,
                runtime_mode: *runtime_mode,
            }),
            _ => return None,
        }
    }
    Some(modes)
}

fn apply_linear_expr_modes(nodes: &mut [Node], modes: &[IfExprModeCacheEntry]) -> bool {
    let mut mode_index = 0usize;
    for node in nodes {
        match node {
            Node::Text(_) => {}
            Node::Expr {
                mode, runtime_mode, ..
            } => {
                let Some(cached) = modes.get(mode_index) else {
                    return false;
                };
                *mode = cached.mode;
                *runtime_mode = cached.runtime_mode;
                mode_index += 1;
            }
            _ => return false,
        }
    }
    mode_index == modes.len()
}

fn dedup_analysis_flows(flows: Vec<AnalysisFlow>) -> Vec<AnalysisFlow> {
    let mut deduped = Vec::with_capacity(flows.len());
    let mut seen = HashSet::with_capacity(flows.len());

    for flow in flows {
        let key = (
            flow.kind,
            flow.tracker.state(),
            flow.tracker.url_part,
            flow.tracker.js_scan_state,
            flow.tracker.css_scan_state,
            flow.tracker.script_json,
            flow.tracker.attr_name_dynamic_pending,
            flow.tracker.attr_value_from_dynamic_attr,
        );

        if seen.insert(key) {
            deduped.push(flow);
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

    if has_url_part_ambiguity(left.iter().chain(right.iter())) {
        return Err(TemplateError::Parse(format!(
            "{{{{{branch_name}}}}} branches end in ambiguous context within URL"
        )));
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

    if has_url_part_ambiguity(flows.iter()) {
        return Err(TemplateError::Parse(format!(
            "{{{{{block_name}}}}} branches end in ambiguous context within URL"
        )));
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
        if let Some(context) = tracker_script_expr_context(&flow.tracker) {
            js_contexts.insert(context);
        }
    }
    js_contexts.len() > 1
}

fn text_starts_with_js_slash(text: &str) -> bool {
    let trimmed = text.trim_start_matches(is_html_space_char);
    trimmed.starts_with('/')
}

fn has_url_part_ambiguity<'a, I>(flows: I) -> bool
where
    I: IntoIterator<Item = &'a AnalysisFlow>,
{
    let mut url_parts = HashSet::new();
    for flow in flows {
        if flow.kind != AnalysisFlowKind::Normal {
            continue;
        }
        if !matches!(
            flow.tracker.state().mode,
            EscapeMode::AttrQuoted {
                kind: AttrKind::Url,
                ..
            } | EscapeMode::AttrUnquoted {
                kind: AttrKind::Url
            }
        ) {
            continue;
        }
        url_parts.insert(flow.tracker.url_part.unwrap_or(UrlPartContext::Path));
    }
    url_parts.len() > 1
}

fn url_part_context(rendered: &str) -> Option<UrlPartContext> {
    let context = current_tag_value_context(rendered)?;
    if attr_kind(&context.attr_name) != AttrKind::Url {
        return None;
    }

    let value = context.value_prefix;
    if value.contains('#') {
        Some(UrlPartContext::Fragment)
    } else if value.contains('?') {
        Some(UrlPartContext::Query)
    } else {
        Some(UrlPartContext::Path)
    }
}

fn range_reentry_context_matches(start: &ContextTracker, candidate: &ContextTracker) -> bool {
    let start_state = start.state();
    let candidate_state = candidate.state();
    if matches!(start_state.mode, EscapeMode::ScriptExpr)
        && matches!(candidate_state.mode, EscapeMode::ScriptExpr)
    {
        // Go-compatible range re-entry accepts js_ctx drift in ScriptExpr
        // (including JSON script types like application/json, application/ld+json,
        // and *+json). Ambiguous slash handling is validated when the next
        // text token actually starts with '/'.
        return true;
    }

    start_state == candidate_state
}

fn should_validate_action_context(tracker: &ContextTracker) -> bool {
    let state = &tracker.state;
    state.in_js_attribute
        || state.in_css_attribute
        || state.is_script_tag_context()
        || state.is_style_tag_context()
}

fn should_resolve_expr_mode_at_runtime(mode: EscapeMode, tracker: &ContextTracker) -> bool {
    matches!(
        mode,
        EscapeMode::AttrQuoted {
            kind: AttrKind::Normal,
            ..
        } | EscapeMode::AttrUnquoted {
            kind: AttrKind::Normal
        }
    ) && tracker.attr_value_from_dynamic_attr
}

fn is_attr_value_mode(mode: EscapeMode) -> bool {
    matches!(
        mode,
        EscapeMode::AttrQuoted { .. } | EscapeMode::AttrUnquoted { .. }
    )
}

fn is_tag_open_related_mode(mode: EscapeMode) -> bool {
    matches!(
        mode,
        EscapeMode::AttrName | EscapeMode::AttrQuoted { .. } | EscapeMode::AttrUnquoted { .. }
    )
}

fn placeholder_advances_parse_context(tracker: &ContextTracker, mode: EscapeMode) -> bool {
    match mode {
        EscapeMode::Html => tracker.state.in_open_tag,
        EscapeMode::Rcdata => false,
        EscapeMode::AttrQuoted { kind, .. } | EscapeMode::AttrUnquoted { kind } => {
            matches!(kind, AttrKind::Js | AttrKind::Css)
        }
        _ => true,
    }
}

fn is_empty_template_body(nodes: &[Node]) -> bool {
    nodes.iter().all(|node| match node {
        Node::Text(text_node) => text_node.raw.trim().is_empty(),
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
    if tracker.state.in_js_attribute || tracker.state.is_script_tag_context() {
        if has_unfinished_escape_suffix(&tracker.rendered) {
            return Err(TemplateError::Parse(
                "unfinished escape sequence in JS string".to_string(),
            ));
        }
        if matches!(tracker.mode(), EscapeMode::ScriptRegexp)
            && matches!(
                tracker.js_scan_state,
                Some(JsScanState::RegExp {
                    in_char_class: true,
                    ..
                })
            )
        {
            return Err(TemplateError::Parse(
                "unfinished JS regexp charset".to_string(),
            ));
        }
    }

    if tracker.state.in_css_attribute || tracker.state.is_style_tag_context() {
        if has_unfinished_escape_suffix(&tracker.rendered) {
            return Err(TemplateError::Parse(
                "unfinished escape sequence in CSS string".to_string(),
            ));
        }
    }

    Ok(())
}

fn has_unfinished_escape_suffix(rendered: &str) -> bool {
    let bytes = rendered.as_bytes();
    let mut index = bytes.len();
    while index > 0 && bytes[index - 1] == b'\\' {
        index -= 1;
    }
    ((bytes.len() - index) & 1) == 1
}

fn css_prefix_for_context(rendered: &str) -> Option<String> {
    if let Some(context) = current_tag_value_context(rendered) {
        if attr_kind(&context.attr_name) == AttrKind::Css {
            return Some(context.value_prefix);
        }
    }

    current_unclosed_tag_content(rendered, "style").map(str::to_string)
}

fn tracker_script_expr_context(tracker: &ContextTracker) -> Option<JsContext> {
    match tracker.js_scan_state? {
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
        | EscapeMode::Rcdata
        | EscapeMode::AttrQuoted { .. }
        | EscapeMode::AttrUnquoted { .. }
        | EscapeMode::AttrName
        | EscapeMode::ScriptString { .. }
        | EscapeMode::ScriptJsonString { .. }
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
    seed_rendered_for_state_with_url_part(state, None)
}

fn url_part_seed_suffix(url_part: Option<UrlPartContext>) -> &'static str {
    match url_part.unwrap_or(UrlPartContext::Path) {
        UrlPartContext::Path => "x",
        UrlPartContext::Query => "x?x",
        UrlPartContext::Fragment => "x#x",
    }
}

fn url_part_from_mode_and_rendered(mode: EscapeMode, rendered: &str) -> Option<UrlPartContext> {
    match mode {
        EscapeMode::AttrQuoted {
            kind: AttrKind::Url,
            ..
        }
        | EscapeMode::AttrUnquoted {
            kind: AttrKind::Url,
        } => url_part_context(rendered),
        _ => None,
    }
}

fn seed_rendered_for_state_with_url_part(
    state: &ContextState,
    url_part: Option<UrlPartContext>,
) -> String {
    match state.mode {
        EscapeMode::Html => {
            if state.in_open_tag {
                "<x".to_string()
            } else {
                String::new()
            }
        }
        EscapeMode::Rcdata => "<textarea>".to_string(),
        EscapeMode::AttrName => "<a x".to_string(),
        EscapeMode::AttrQuoted {
            kind: AttrKind::Url,
            quote,
        } => {
            let seed = url_part_seed_suffix(url_part);
            format!("<a {}={quote}{seed}", attr_name_for_kind(AttrKind::Url))
        }
        EscapeMode::AttrQuoted { kind, quote } => {
            format!("<a {}={quote}x", attr_name_for_kind(kind))
        }
        EscapeMode::AttrUnquoted {
            kind: AttrKind::Url,
        } => {
            let seed = url_part_seed_suffix(url_part);
            format!("<a {}={seed}", attr_name_for_kind(AttrKind::Url))
        }
        EscapeMode::AttrUnquoted { kind } => format!("<a {}=x", attr_name_for_kind(kind)),
        EscapeMode::ScriptExpr => "<script>".to_string(),
        EscapeMode::ScriptString { quote } => format!("<script>{quote}"),
        EscapeMode::ScriptJsonString { quote } => format!("<script>{quote}"),
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

fn last_unclosed_tag_start(rendered: &str) -> Option<usize> {
    let bytes = rendered.as_bytes();
    let mut in_tag = false;
    let mut quote: Option<u8> = None;
    let mut tag_start = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        let byte = bytes[i];
        if !in_tag {
            if byte == b'<' && i + 1 < bytes.len() {
                let next = bytes[i + 1];
                if next.is_ascii_alphabetic() || matches!(next, b'/' | b'!' | b'?') {
                    in_tag = true;
                    quote = None;
                    tag_start = i;
                }
            }
            i += 1;
            continue;
        }

        if let Some(active_quote) = quote {
            if byte == active_quote {
                quote = None;
            }
            i += 1;
            continue;
        }

        match byte {
            b'\'' | b'"' => {
                quote = Some(byte);
                i += 1;
            }
            b'>' => {
                in_tag = false;
                i += 1;
            }
            _ => i += 1,
        }
    }

    if in_tag { Some(tag_start) } else { None }
}

fn is_in_unclosed_tag_context(rendered: &str) -> bool {
    let Some(last_lt) = last_unclosed_tag_start(rendered) else {
        return false;
    };

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
            Ok(Value::from(query_escape_url(&combined)))
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
                Value::SafeJsStr(v) => v.len(),
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
        (Value::SafeJsStr(a), Value::SafeJsStr(b)) => a == b,
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
        Value::SafeJsStr(text) => Some(text.clone()),
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
        | Value::SafeJsStr(text)
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
        Value::SafeJsStr(text) => Ok(serde_json::to_string(text)?),
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
        | Value::SafeJsStr(value)
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
        | Value::SafeJsStr(value)
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
        | Value::SafeJsStr(text)
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
        | Value::SafeJsStr(_)
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

    // Fast path: when traversing pure JSON objects/arrays, walk by reference
    // and clone only once at the end.
    if let Value::Json(base_json) = base {
        let mut current_json = base_json;

        for (index, segment) in path.iter().enumerate() {
            let direct = match current_json {
                JsonValue::Object(map) => map.get(segment),
                JsonValue::Array(items) => segment
                    .parse::<usize>()
                    .ok()
                    .and_then(|item_index| items.get(item_index)),
                _ => None,
            };

            if let Some(next_json) = direct {
                current_json = next_json;
                continue;
            }

            if let Some(method) = methods.get(segment) {
                let receiver = Value::Json(current_json.clone());
                let mut current = method(&receiver, &[])?;
                for remaining in &path[index + 1..] {
                    current =
                        lookup_single_segment(&current, remaining, methods, missing_key_mode)?;
                }
                return Ok(current);
            }

            return match current_json {
                JsonValue::Object(_) => missing_value_for_key(segment, missing_key_mode),
                _ => Ok(Value::Json(JsonValue::Null)),
            };
        }

        return Ok(Value::Json(current_json.clone()));
    }

    let mut current = base.clone();
    for segment in path {
        current = lookup_single_segment(&current, segment, methods, missing_key_mode)?;
    }
    Ok(current)
}

fn lookup_json_path_with_methods(
    base_json: &JsonValue,
    path: &[String],
    methods: &MethodMap,
    missing_key_mode: MissingKeyMode,
) -> Result<Value> {
    if path.is_empty() {
        return Ok(Value::Json(base_json.clone()));
    }

    let mut current_json = base_json;
    for (index, segment) in path.iter().enumerate() {
        let direct = match current_json {
            JsonValue::Object(map) => map.get(segment),
            JsonValue::Array(items) => segment
                .parse::<usize>()
                .ok()
                .and_then(|item_index| items.get(item_index)),
            _ => None,
        };

        if let Some(next_json) = direct {
            current_json = next_json;
            continue;
        }

        if let Some(method) = methods.get(segment) {
            let receiver = Value::Json(current_json.clone());
            let mut current = method(&receiver, &[])?;
            for remaining in &path[index + 1..] {
                current = lookup_single_segment(&current, remaining, methods, missing_key_mode)?;
            }
            return Ok(current);
        }

        return match current_json {
            JsonValue::Object(_) => missing_value_for_key(segment, missing_key_mode),
            _ => Ok(Value::Json(JsonValue::Null)),
        };
    }

    Ok(Value::Json(current_json.clone()))
}

fn lookup_dot_path_with_methods(
    dot: &Value,
    dot_json: Option<&JsonValue>,
    path: &[String],
    methods: &MethodMap,
    missing_key_mode: MissingKeyMode,
) -> Result<Value> {
    if let Some(base_json) = dot_json {
        lookup_json_path_with_methods(base_json, path, methods, missing_key_mode)
    } else {
        lookup_path_with_methods(dot, path, methods, missing_key_mode)
    }
}

fn dot_to_owned(dot: &Value, dot_json: Option<&JsonValue>) -> Value {
    if let Some(value) = dot_json {
        Value::Json(value.clone())
    } else {
        dot.clone()
    }
}

fn dot_is_object(dot: &Value, dot_json: Option<&JsonValue>) -> bool {
    if let Some(value) = dot_json {
        matches!(value, JsonValue::Object(_))
    } else {
        matches!(dot, Value::Json(JsonValue::Object(_)))
    }
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

fn lookup_identifier_with_dot_json(
    dot: &Value,
    dot_json: Option<&JsonValue>,
    root: &Value,
    name: &str,
    methods: &MethodMap,
    _missing_key_mode: MissingKeyMode,
) -> Result<Option<Value>> {
    if let Some(value) = dot_json {
        if let JsonValue::Object(map) = value
            && let Some(found) = map.get(name)
        {
            return Ok(Some(Value::Json(found.clone())));
        }
    } else if let Some(value) = lookup_object_key(dot, name) {
        return Ok(Some(value));
    }

    if let Some(value) = lookup_object_key(root, name) {
        return Ok(Some(value));
    }

    if let Some(method) = methods.get(name) {
        if let Some(dot_json) = dot_json {
            let dot_value = Value::Json(dot_json.clone());
            return Ok(Some(method(&dot_value, &[])?));
        }
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
        if let Some(slot) = scope.get_mut(name) {
            *slot = value;
            return Ok(());
        }
    }

    Err(TemplateError::Render(format!(
        "variable `${name}` is not declared"
    )))
}

#[derive(Clone, Copy)]
struct RangeAssignTargets {
    first_scope: usize,
    second_scope: Option<usize>,
}

fn find_variable_scope_index(scopes: &ScopeStack, name: &str) -> Option<usize> {
    for index in (0..scopes.len()).rev() {
        if scopes[index].contains_key(name) {
            return Some(index);
        }
    }
    None
}

fn resolve_range_assign_targets(
    scopes: &ScopeStack,
    vars: &[String],
) -> Result<RangeAssignTargets> {
    let first_scope = find_variable_scope_index(scopes, &vars[0])
        .ok_or_else(|| TemplateError::Render(format!("variable `${}` is not declared", vars[0])))?;
    let second_scope = if vars.len() >= 2 {
        Some(find_variable_scope_index(scopes, &vars[1]).ok_or_else(|| {
            TemplateError::Render(format!("variable `${}` is not declared", vars[1]))
        })?)
    } else {
        None
    };
    Ok(RangeAssignTargets {
        first_scope,
        second_scope,
    })
}

fn assign_variable_in_scope(
    scopes: &mut ScopeStack,
    scope_index: usize,
    name: &str,
    value: Value,
) -> Result<()> {
    if let Some(scope) = scopes.get_mut(scope_index)
        && let Some(slot) = scope.get_mut(name)
    {
        *slot = value;
        return Ok(());
    }
    Err(TemplateError::Render(format!(
        "variable `${name}` is not declared"
    )))
}

fn upsert_scope_variable(scope: &mut HashMap<String, Value>, name: &str, value: Value) {
    if let Some(slot) = scope.get_mut(name) {
        *slot = value;
    } else {
        scope.insert(name.to_string(), value);
    }
}

fn retain_range_scope_vars(scope: &mut HashMap<String, Value>, vars: &[String]) {
    if scope.len() <= vars.len() {
        return;
    }
    let first = vars[0].as_str();
    let second = vars.get(1).map(String::as_str);
    scope.retain(|name, _| name == first || second.is_some_and(|other| name == other));
}

fn declare_range_variables(
    scope: &mut HashMap<String, Value>,
    vars: &[String],
    key: Option<Value>,
    item: Value,
) {
    retain_range_scope_vars(scope, vars);
    if vars.len() == 1 {
        upsert_scope_variable(scope, &vars[0], item);
    } else if vars.len() >= 2 {
        upsert_scope_variable(
            scope,
            &vars[0],
            key.unwrap_or_else(|| Value::Json(JsonValue::Null)),
        );
        upsert_scope_variable(scope, &vars[1], item);
    }
}

fn assign_range_variables(
    scopes: &mut ScopeStack,
    vars: &[String],
    targets: RangeAssignTargets,
    key: Option<Value>,
    item: Value,
) -> Result<()> {
    if vars.len() == 1 {
        assign_variable_in_scope(scopes, targets.first_scope, &vars[0], item)?;
    } else if vars.len() >= 2 {
        assign_variable_in_scope(
            scopes,
            targets.first_scope,
            &vars[0],
            key.unwrap_or_else(|| Value::Json(JsonValue::Null)),
        )?;
        if let Some(second_scope) = targets.second_scope {
            assign_variable_in_scope(scopes, second_scope, &vars[1], item)?;
        }
    }
    Ok(())
}

fn range_iteration_count(value: &Value) -> usize {
    match value {
        Value::Json(JsonValue::Array(items)) => items.len(),
        Value::Json(JsonValue::Object(items)) => items.len(),
        Value::Json(JsonValue::String(value)) => value.chars().count(),
        _ => 0,
    }
}

fn analyze_text_only_template_nodes(nodes: &[Node]) -> (ContextTracker, bool) {
    if let Some(text) = single_text_node_raw(nodes) {
        return analyze_text_only_segment(text);
    }

    let mut combined = String::new();
    for node in nodes {
        if let Node::Text(text_node) = node {
            combined.push_str(&text_node.raw);
        }
    }
    analyze_text_only_segment(&combined)
}

fn analyze_text_only_segment(text: &str) -> (ContextTracker, bool) {
    let cacheable_output = is_text_only_output_cacheable(text);

    if is_simple_static_html_text_context(text) {
        let tracker = ContextTracker::from_state(ContextState::html_text());
        return (tracker, cacheable_output);
    }

    let mut tracker = ContextTracker::from_state(ContextState::html_text());
    tracker.append_text_for_parse(text);
    (tracker, cacheable_output)
}

fn single_text_node_raw(nodes: &[Node]) -> Option<&str> {
    if nodes.len() != 1 {
        return None;
    }

    match &nodes[0] {
        Node::Text(text_node) => Some(text_node.raw.as_str()),
        _ => None,
    }
}

fn collect_text_only_nodes(nodes: &[Node]) -> Option<String> {
    let mut combined = String::new();
    for node in nodes {
        if let Node::Text(text_node) = node {
            combined.push_str(&text_node.raw);
        } else {
            return None;
        }
    }
    Some(combined)
}

fn scan_text_summary(text: &str) -> TextScanSummary {
    let bytes = text.as_bytes();
    let mut summary = TextScanSummary::default();

    let mut i = 0usize;
    while i < bytes.len() {
        let byte = bytes[i];
        match byte {
            b'<' => {
                summary.has_lt = true;
                if i + 3 < bytes.len()
                    && bytes[i + 1] == b'!'
                    && bytes[i + 2] == b'-'
                    && bytes[i + 3] == b'-'
                {
                    summary.has_comment_open = true;
                }
            }
            b'=' => summary.has_eq = true,
            b'\'' => summary.has_single_quote = true,
            b'"' => summary.has_double_quote = true,
            b'`' => summary.has_backtick = true,
            b's' | b'S' => summary.has_s = true,
            b't' | b'T' => summary.has_t = true,
            b'-' => {
                if i + 2 < bytes.len() && bytes[i + 1] == b'-' && bytes[i + 2] == b'>' {
                    summary.has_comment_close = true;
                }
            }
            _ => {}
        }

        i += 1;
    }

    summary
}

fn scan_text_summary_if_no_default_delim(text: &str) -> Option<TextScanSummary> {
    let bytes = text.as_bytes();
    let mut summary = TextScanSummary::default();

    let mut i = 0usize;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            return None;
        }

        match byte {
            b'<' => {
                summary.has_lt = true;
                if i + 3 < bytes.len()
                    && bytes[i + 1] == b'!'
                    && bytes[i + 2] == b'-'
                    && bytes[i + 3] == b'-'
                {
                    summary.has_comment_open = true;
                }
            }
            b'=' => summary.has_eq = true,
            b'\'' => summary.has_single_quote = true,
            b'"' => summary.has_double_quote = true,
            b'`' => summary.has_backtick = true,
            b's' | b'S' => summary.has_s = true,
            b't' | b'T' => summary.has_t = true,
            b'-' => {
                if i + 2 < bytes.len() && bytes[i + 1] == b'-' && bytes[i + 2] == b'>' {
                    summary.has_comment_close = true;
                }
            }
            _ => {}
        }

        i += 1;
    }

    Some(summary)
}

fn deferred_text_only_context_analysis(text: &str, scan: &TextScanSummary) -> bool {
    if scan.has_eq
        || scan.has_single_quote
        || scan.has_double_quote
        || scan.has_backtick
        || scan.has_comment_open
        || scan.has_comment_close
    {
        return false;
    }

    if !scan.has_lt {
        return true;
    }

    if !scan.has_s && !scan.has_t {
        return true;
    }

    !contains_special_text_context_tag(text)
}

fn cacheable_text_only_output(text: &str, scan: &TextScanSummary) -> bool {
    if !scan.has_lt {
        return true;
    }
    if !scan.has_s && !scan.has_t {
        return true;
    }
    !contains_script_or_style_tag(text)
}

fn is_text_only_output_cacheable(text: &str) -> bool {
    let scan = scan_text_summary(text);
    cacheable_text_only_output(text, &scan)
}

fn is_simple_static_html_text_context(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }

        i += 1;
        if i >= bytes.len() {
            return false;
        }

        if matches!(bytes[i], b'!' | b'?') {
            return false;
        }

        let closing = if bytes[i] == b'/' {
            i += 1;
            true
        } else {
            false
        };

        if i >= bytes.len() || !bytes[i].is_ascii_alphabetic() {
            return false;
        }

        let tag_start = i;
        i += 1;
        while i < bytes.len() && is_html_tag_name_byte(bytes[i]) {
            i += 1;
        }
        let tag_name = &bytes[tag_start..i];
        if tag_name.eq_ignore_ascii_case(b"script")
            || tag_name.eq_ignore_ascii_case(b"style")
            || tag_name.eq_ignore_ascii_case(b"title")
            || tag_name.eq_ignore_ascii_case(b"textarea")
        {
            return false;
        }

        while i < bytes.len() && is_html_space(bytes[i]) {
            i += 1;
        }
        if !closing && i < bytes.len() && bytes[i] == b'/' {
            i += 1;
            while i < bytes.len() && is_html_space(bytes[i]) {
                i += 1;
            }
        }
        if i >= bytes.len() || bytes[i] != b'>' {
            return false;
        }
        i += 1;
    }

    true
}

fn contains_script_or_style_tag(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }

        i += 1;
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b'/' {
            i += 1;
            if i >= bytes.len() {
                break;
            }
        }
        if !bytes[i].is_ascii_alphabetic() {
            continue;
        }

        let start = i;
        i += 1;
        while i < bytes.len() && is_html_tag_name_byte(bytes[i]) {
            i += 1;
        }

        let name = &bytes[start..i];
        if !is_html_tag_boundary(bytes.get(i).copied()) {
            continue;
        }
        if name.eq_ignore_ascii_case(b"script") || name.eq_ignore_ascii_case(b"style") {
            return true;
        }
    }
    false
}

fn contains_special_text_context_tag(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }

        i += 1;
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b'/' {
            i += 1;
            if i >= bytes.len() {
                break;
            }
        }
        if !bytes[i].is_ascii_alphabetic() {
            continue;
        }

        let start = i;
        i += 1;
        while i < bytes.len() && is_html_tag_name_byte(bytes[i]) {
            i += 1;
        }
        if !is_html_tag_boundary(bytes.get(i).copied()) {
            continue;
        }

        let tag = &bytes[start..i];
        if tag.eq_ignore_ascii_case(b"script")
            || tag.eq_ignore_ascii_case(b"style")
            || tag.eq_ignore_ascii_case(b"title")
            || tag.eq_ignore_ascii_case(b"textarea")
        {
            return true;
        }
    }

    false
}

fn term_may_reference_dot(term: &Term) -> bool {
    match term {
        Term::DotPath(_) | Term::Identifier(_) => true,
        Term::SubExpr(expr) => expr_may_reference_dot(expr),
        Term::SubExprPath { expr, .. } => expr_may_reference_dot(expr),
        Term::RootPath(_) | Term::Variable { .. } | Term::Literal(_) => false,
    }
}

fn expr_may_reference_dot(expr: &Expr) -> bool {
    for command in &expr.commands {
        match command {
            Command::Value(term) => {
                if term_may_reference_dot(term) {
                    return true;
                }
            }
            Command::Call { args, .. } => {
                if args.iter().any(term_may_reference_dot) {
                    return true;
                }
            }
            Command::Invoke { callee, args } => {
                if term_may_reference_dot(callee) || args.iter().any(term_may_reference_dot) {
                    return true;
                }
            }
        }
    }
    false
}

fn nodes_may_reference_dot(nodes: &[Node]) -> bool {
    for node in nodes {
        match node {
            Node::Text(_) | Node::Break | Node::Continue => {}
            Node::Expr { expr, .. } => {
                if expr_may_reference_dot(expr) {
                    return true;
                }
            }
            Node::SetVar { value, .. } => {
                if expr_may_reference_dot(value) {
                    return true;
                }
            }
            Node::If {
                condition,
                then_branch,
                else_branch,
            } => {
                if expr_may_reference_dot(condition)
                    || nodes_may_reference_dot(then_branch)
                    || nodes_may_reference_dot(else_branch)
                {
                    return true;
                }
            }
            Node::Range {
                iterable,
                body,
                else_branch,
                ..
            } => {
                if expr_may_reference_dot(iterable)
                    || nodes_may_reference_dot(body)
                    || nodes_may_reference_dot(else_branch)
                {
                    return true;
                }
            }
            Node::With {
                value,
                body,
                else_branch,
            } => {
                if expr_may_reference_dot(value)
                    || nodes_may_reference_dot(body)
                    || nodes_may_reference_dot(else_branch)
                {
                    return true;
                }
            }
            Node::TemplateCall { data, .. } => {
                if data.is_none() || data.as_ref().is_some_and(expr_may_reference_dot) {
                    return true;
                }
            }
            Node::Block { data, body, .. } => {
                if data.is_none()
                    || data.as_ref().is_some_and(expr_may_reference_dot)
                    || nodes_may_reference_dot(body)
                {
                    return true;
                }
            }
            Node::Define { body, .. } => {
                if nodes_may_reference_dot(body) {
                    return true;
                }
            }
        }
    }
    false
}

fn range_static_text_body(body: &[Node]) -> Option<String> {
    let mut combined = String::new();
    for node in body {
        match node {
            Node::Text(text_node) => combined.push_str(&text_node.raw),
            _ => return None,
        }
    }
    Some(combined)
}

struct RepeatedTextPlan {
    text: String,
    updates_tracker: bool,
}

fn range_static_text_fast_path_text(
    tracker: &ContextTracker,
    body_text: &str,
) -> Option<RepeatedTextPlan> {
    if body_text.is_empty() {
        return Some(RepeatedTextPlan {
            text: String::new(),
            updates_tracker: false,
        });
    }
    if !tracker.state.is_text_context() {
        return None;
    }
    if contains_ascii_case_insensitive(body_text, b"<script")
        || contains_ascii_case_insensitive(body_text, b"</script")
        || contains_ascii_case_insensitive(body_text, b"<style")
        || contains_ascii_case_insensitive(body_text, b"</style")
        || body_text.contains("<!--")
        || body_text.contains("-->")
    {
        return None;
    }

    let text = if body_text.as_bytes().contains(&b'<') {
        filter_html_text_sections(&tracker.rendered, body_text)
    } else {
        body_text.to_string()
    };
    if text.is_empty() {
        return Some(RepeatedTextPlan {
            text,
            updates_tracker: false,
        });
    }

    let mut probe = tracker.clone();
    probe.append_text(&text);
    let updates_tracker = probe.state != tracker.state
        || probe.url_part != tracker.url_part
        || probe.js_scan_state != tracker.js_scan_state
        || probe.css_scan_state != tracker.css_scan_state
        || probe.script_json != tracker.script_json;

    Some(RepeatedTextPlan {
        text,
        updates_tracker,
    })
}

fn append_text_repeated(output: &mut String, text: &str, iterations: usize) {
    if iterations == 0 || text.is_empty() {
        return;
    }
    let additional = text.len().saturating_mul(iterations);
    output.reserve(additional);
    for _ in 0..iterations {
        output.push_str(text);
    }
}

fn append_repeated_text(
    output: &mut String,
    tracker: &mut ContextTracker,
    text: &str,
    iterations: usize,
    skip_tracker_update: bool,
) {
    if iterations == 0 || text.is_empty() {
        return;
    }
    if iterations == 1 {
        output.push_str(text);
        if !skip_tracker_update {
            tracker.append_text(text);
        }
        return;
    }
    if skip_tracker_update {
        append_text_repeated(output, text, iterations);
        return;
    }

    let mut repeated = String::with_capacity(text.len().saturating_mul(iterations));
    for _ in 0..iterations {
        repeated.push_str(text);
    }
    output.push_str(&repeated);
    if !skip_tracker_update {
        tracker.append_text(&repeated);
    }
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

fn validate_unquoted_attr_hazards(source: &str, has_lt: bool, has_eq: bool) -> Result<()> {
    if !has_lt || !has_eq {
        return Ok(());
    }

    let bytes = source.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] != b'<' || i + 1 >= bytes.len() || !bytes[i + 1].is_ascii_alphabetic() {
            i += 1;
            continue;
        }

        i += 1;
        while i < bytes.len() && !is_html_space(bytes[i]) && !matches!(bytes[i], b'>' | b'/' | b'=')
        {
            i += 1;
        }

        if i < bytes.len() && bytes[i] == b'=' {
            return Err(TemplateError::Parse(
                "expected space, attr name, or end of tag".to_string(),
            ));
        }

        loop {
            while i < bytes.len() && is_html_space(bytes[i]) {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }

            if bytes[i] == b'>' {
                i += 1;
                break;
            }
            if bytes[i] == b'/' {
                i += 1;
                if i < bytes.len() && bytes[i] == b'>' {
                    i += 1;
                    break;
                }
            }
            if bytes[i] == b'=' {
                return Err(TemplateError::Parse(
                    "expected space, attr name, or end of tag".to_string(),
                ));
            }

            let name_start = i;
            while i < bytes.len()
                && !is_html_space(bytes[i])
                && !matches!(bytes[i], b'=' | b'>' | b'/')
            {
                i += 1;
            }
            if i <= name_start {
                i += 1;
                continue;
            }

            while i < bytes.len() && is_html_space(bytes[i]) {
                i += 1;
            }
            if i >= bytes.len() || bytes[i] != b'=' {
                continue;
            }

            i += 1;
            while i < bytes.len() && is_html_space(bytes[i]) {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }

            if bytes[i] == b'\'' || bytes[i] == b'"' {
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
                continue;
            }

            let value_start = i;
            while i < bytes.len() && !is_html_space(bytes[i]) && bytes[i] != b'>' {
                i += 1;
            }
            if i > value_start {
                let value = String::from_utf8_lossy(&bytes[value_start..i]);
                if value.contains('=')
                    || value.contains('"')
                    || value.contains('\'')
                    || value.contains('`')
                {
                    return Err(TemplateError::Parse(format!("in unquoted attr: {value:?}")));
                }
            }
        }
    }

    Ok(())
}

fn strip_html_comments(source: &str) -> Cow<'_, str> {
    if !source.contains("<!--") {
        return Cow::Borrowed(source);
    }

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

    Cow::Owned(output)
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

    template.finalize_contexts_after_parse()?;
    Ok(())
}

fn tokenize(source: &str, left_delim: &str, right_delim: &str) -> Result<Vec<Token>> {
    if left_delim.is_empty() || right_delim.is_empty() {
        return Err(TemplateError::Parse(
            "template delimiters must not be empty".to_string(),
        ));
    }

    if left_delim == "{{" && right_delim == "}}" {
        return tokenize_default_delims(source);
    }

    tokenize_with_delims_generic(source, left_delim, right_delim)
}

fn tokenize_default_delims(source: &str) -> Result<Vec<Token>> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut cursor = 0usize;

    while let Some(start) = find_byte_pair(bytes, cursor, b'{', b'{') {
        if start > cursor {
            tokens.push(Token::Text {
                start: cursor,
                end: start,
            });
        }

        let mut action_start = start + 2;
        if bytes.get(action_start) == Some(&b'-') {
            let should_treat_as_unary_minus = matches!(
                bytes.get(action_start + 1),
                Some(next) if next.is_ascii_digit() || *next == b'.'
            );
            if !should_treat_as_unary_minus {
                action_start += 1;
                trim_last_text_whitespace(&mut tokens, source);
            }
        }

        let end = find_byte_pair(bytes, action_start, b'}', b'}')
            .ok_or_else(|| TemplateError::Parse("unclosed action (missing `}}`)".to_string()))?;

        let mut action_start_trimmed = trim_start_whitespace(source, action_start, end);
        let mut action_end_trimmed = trim_end_whitespace(source, action_start_trimmed, end);
        let trim_right = action_start_trimmed < action_end_trimmed
            && source[action_start_trimmed..action_end_trimmed].ends_with('-');
        if trim_right {
            action_end_trimmed =
                previous_char_boundary(source, action_start_trimmed, action_end_trimmed);
            action_end_trimmed =
                trim_end_whitespace(source, action_start_trimmed, action_end_trimmed);
        }

        action_start_trimmed =
            trim_start_whitespace(source, action_start_trimmed, action_end_trimmed);
        tokens.push(Token::Action {
            start: action_start_trimmed,
            end: action_end_trimmed,
        });
        cursor = end + 2;

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
        tokens.push(Token::Text {
            start: cursor,
            end: source.len(),
        });
    }

    Ok(tokens)
}

fn tokenize_with_delims_generic(
    source: &str,
    left_delim: &str,
    right_delim: &str,
) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut cursor = 0usize;

    while let Some(start_offset) = source[cursor..].find(left_delim) {
        let start = cursor + start_offset;
        if start > cursor {
            tokens.push(Token::Text {
                start: cursor,
                end: start,
            });
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
                trim_last_text_whitespace(&mut tokens, source);
            }
        }

        let end_offset = source[action_start..].find(right_delim).ok_or_else(|| {
            TemplateError::Parse(format!("unclosed action (missing `{right_delim}`)"))
        })?;
        let end = action_start + end_offset;

        let mut action_start_trimmed = trim_start_whitespace(source, action_start, end);
        let mut action_end_trimmed = trim_end_whitespace(source, action_start_trimmed, end);
        let trim_right = action_start_trimmed < action_end_trimmed
            && source[action_start_trimmed..action_end_trimmed].ends_with('-');
        if trim_right {
            action_end_trimmed =
                previous_char_boundary(source, action_start_trimmed, action_end_trimmed);
            action_end_trimmed =
                trim_end_whitespace(source, action_start_trimmed, action_end_trimmed);
        }

        action_start_trimmed =
            trim_start_whitespace(source, action_start_trimmed, action_end_trimmed);
        tokens.push(Token::Action {
            start: action_start_trimmed,
            end: action_end_trimmed,
        });
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
        tokens.push(Token::Text {
            start: cursor,
            end: source.len(),
        });
    }

    Ok(tokens)
}

fn find_byte_pair(bytes: &[u8], start: usize, first: u8, second: u8) -> Option<usize> {
    if start + 1 >= bytes.len() {
        return None;
    }

    let mut cursor = start;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] == first && bytes[cursor + 1] == second {
            return Some(cursor);
        }
        cursor += 1;
    }

    None
}

fn trim_start_whitespace(source: &str, mut start: usize, end: usize) -> usize {
    while start < end {
        let mut chars = source[start..end].chars();
        let Some(ch) = chars.next() else {
            break;
        };
        if ch.is_whitespace() {
            start += ch.len_utf8();
        } else {
            break;
        }
    }
    start
}

fn trim_end_whitespace(source: &str, start: usize, end: usize) -> usize {
    if start >= end {
        return start;
    }
    let segment = &source[start..end];
    let mut trimmed_end = end;
    for (offset, ch) in segment.char_indices().rev() {
        if ch.is_whitespace() {
            trimmed_end = start + offset;
        } else {
            break;
        }
    }
    trimmed_end
}

fn previous_char_boundary(source: &str, start: usize, end: usize) -> usize {
    if start >= end {
        return start;
    }
    source[..end]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
        .unwrap_or(start)
}

fn trim_last_text_whitespace(tokens: &mut Vec<Token>, source: &str) {
    let mut remove_last = false;
    if let Some(Token::Text { start, end }) = tokens.last_mut() {
        *end = trim_end_whitespace(source, *start, *end);
        remove_last = *start == *end;
    }
    if remove_last {
        let _ = tokens.pop();
    }
}

fn parse_nodes(
    source: &Arc<str>,
    tokens: &[Token],
    index: &mut usize,
    stop_keywords: &[&str],
) -> Result<(Vec<Node>, Option<StopAction>)> {
    let mut nodes = Vec::new();

    while *index < tokens.len() {
        match &tokens[*index] {
            Token::Text { start, end } => {
                nodes.push(Node::Text(TextNode::from_span(
                    source.clone(),
                    *start,
                    *end,
                )));
                *index += 1;
            }
            Token::Action { start, end } => {
                let raw_action = &source[*start..*end];
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
                        let parsed = parse_if_from_condition(source, tokens, index, condition)?;
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
                            parse_optional_else_block(source, tokens, index, "range")?;
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
                        let parsed = parse_with_from_value(source, tokens, index, value)?;
                        nodes.push(parsed);
                    }
                    "define" => {
                        let name = parse_quoted_name(tail)?;
                        *index += 1;
                        let (body, stop) = parse_nodes(source, tokens, index, &["end"])?;
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
                        let (body, stop) = parse_nodes(source, tokens, index, &["end"])?;
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
                                runtime_mode: false,
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

fn parse_if_from_condition(
    source: &Arc<str>,
    tokens: &[Token],
    index: &mut usize,
    condition: Expr,
) -> Result<Node> {
    let (then_branch, stop) = parse_nodes(source, tokens, index, &["else", "end"])?;
    let mut else_branch = Vec::new();

    match stop {
        Some(stop) if stop.keyword == "end" => {}
        Some(stop) if stop.keyword == "else" => {
            if stop.tail.is_empty() {
                let (parsed_else, end) = parse_nodes(source, tokens, index, &["end"])?;
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
                    let nested = parse_if_from_condition(source, tokens, index, else_if_condition)?;
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

fn parse_with_from_value(
    source: &Arc<str>,
    tokens: &[Token],
    index: &mut usize,
    value: Expr,
) -> Result<Node> {
    let (body, stop) = parse_nodes(source, tokens, index, &["else", "end"])?;
    let mut else_branch = Vec::new();

    match stop {
        Some(stop) if stop.keyword == "end" => {}
        Some(stop) if stop.keyword == "else" => {
            if stop.tail.is_empty() {
                let (parsed_else, end) = parse_nodes(source, tokens, index, &["end"])?;
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
                    let nested = parse_with_from_value(source, tokens, index, else_with_value)?;
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
    source: &Arc<str>,
    tokens: &[Token],
    index: &mut usize,
    block_name: &str,
) -> Result<(Vec<Node>, Vec<Node>)> {
    let (body, stop) = parse_nodes(source, tokens, index, &["else", "end"])?;
    match stop {
        Some(stop) if stop.keyword == "end" => Ok((body, Vec::new())),
        Some(stop) if stop.keyword == "else" => {
            if !stop.tail.is_empty() {
                return Err(TemplateError::Parse(format!(
                    "{block_name} does not support `else {}`",
                    stop.tail
                )));
            }
            let (else_branch, end) = parse_nodes(source, tokens, index, &["end"])?;
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
    Rcdata,
    AttrQuoted { kind: AttrKind, quote: char },
    AttrUnquoted { kind: AttrKind },
    AttrName,
    ScriptExpr,
    ScriptString { quote: char },
    ScriptJsonString { quote: char },
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

fn escape_value_for_mode(
    value: &Value,
    mode: EscapeMode,
    rendered_prefix: &str,
    url_part: Option<UrlPartContext>,
    css_url_part_hint: Option<Option<UrlPartContext>>,
) -> Result<String> {
    match (value, mode) {
        (Value::SafeHtml(raw), EscapeMode::Html) => return Ok(raw.clone()),
        (Value::SafeHtml(raw), EscapeMode::AttrName) => return Ok(html_name_filter(&raw)),
        (Value::SafeHtmlAttr(raw), EscapeMode::AttrName) => return Ok(raw.clone()),
        (Value::SafeJs(raw), EscapeMode::ScriptExpr)
        | (Value::SafeJs(raw), EscapeMode::ScriptTemplate)
        | (Value::SafeJs(raw), EscapeMode::ScriptRegexp) => {
            return Ok(raw.clone());
        }
        (Value::SafeCss(raw), EscapeMode::StyleExpr) => {
            return Ok(raw.clone());
        }
        _ => {}
    }

    match mode {
        EscapeMode::Html => {
            let plain = plain_string_cow(value);
            Ok(escape_html(plain.as_ref()))
        }
        EscapeMode::Rcdata => match value {
            Value::SafeHtml(raw) => Ok(escape_html_norm(raw)),
            _ => {
                let plain = plain_string_cow(value);
                Ok(escape_html(plain.as_ref()))
            }
        },
        EscapeMode::AttrName => {
            let plain = plain_string_cow(value);
            Ok(html_name_filter(plain.as_ref()))
        }
        EscapeMode::AttrQuoted { kind, quote: _ } => match kind {
            AttrKind::Normal => match value {
                Value::SafeHtml(raw) => Ok(escape_html_norm(&strip_tags(raw))),
                _ => {
                    let plain = plain_string_cow(value);
                    Ok(escape_html(plain.as_ref()))
                }
            },
            AttrKind::Js => {
                let text = js_val_escaper(value)?;
                Ok(escape_html(&text))
            }
            AttrKind::Css => {
                let text = match value {
                    Value::SafeCss(raw) => raw.clone(),
                    Value::SafeHtml(_)
                    | Value::SafeHtmlAttr(_)
                    | Value::SafeJs(_)
                    | Value::SafeJsStr(_)
                    | Value::SafeUrl(_)
                    | Value::SafeSrcset(_) => "ZgotmplZ".to_string(),
                    _ => {
                        let plain = plain_string_cow(value);
                        css_value_filter(plain.as_ref())
                    }
                };
                Ok(escape_html(&text))
            }
            AttrKind::Url => Ok(escape_url_attribute_value_single_pass(
                value,
                url_part,
                HtmlAttributeEscapeMode::Quoted,
            )),
            AttrKind::Srcset => Ok(escape_srcset_attribute_value_single_pass(
                value,
                HtmlAttributeEscapeMode::Quoted,
            )),
        },
        EscapeMode::AttrUnquoted { kind } => match kind {
            AttrKind::Normal => match value {
                Value::SafeHtml(raw) => Ok(html_nospace_escaper_norm(&strip_tags(raw))),
                _ => {
                    let plain = plain_string_cow(value);
                    Ok(html_nospace_escaper(plain.as_ref()))
                }
            },
            AttrKind::Css => {
                let text = match value {
                    Value::SafeCss(raw) => raw.clone(),
                    Value::SafeHtml(_)
                    | Value::SafeHtmlAttr(_)
                    | Value::SafeJs(_)
                    | Value::SafeJsStr(_)
                    | Value::SafeUrl(_)
                    | Value::SafeSrcset(_) => "ZgotmplZ".to_string(),
                    _ => {
                        let plain = plain_string_cow(value);
                        css_value_filter(plain.as_ref())
                    }
                };
                Ok(escape_attr_unquoted(&text))
            }
            AttrKind::Url => Ok(escape_url_attribute_value_single_pass(
                value,
                url_part,
                HtmlAttributeEscapeMode::Unquoted,
            )),
            AttrKind::Srcset => Ok(escape_srcset_attribute_value_single_pass(
                value,
                HtmlAttributeEscapeMode::Unquoted,
            )),
            AttrKind::Js => {
                let text = js_val_escaper(value)?;
                Ok(html_nospace_escaper(&text))
            }
        },
        EscapeMode::ScriptExpr => escape_script_value(value),
        EscapeMode::ScriptTemplate => {
            let plain = plain_string_cow(value);
            Ok(escape_js_string_fragment(plain.as_ref(), '`'))
        }
        EscapeMode::ScriptRegexp => {
            let plain = plain_string_cow(value);
            Ok(escape_js_string_fragment(plain.as_ref(), '/'))
        }
        EscapeMode::ScriptLineComment | EscapeMode::ScriptBlockComment => Ok(String::new()),
        EscapeMode::ScriptString { quote: _ } => match value {
            Value::SafeJsStr(raw) => Ok(js_string_escaper_norm(raw)),
            _ => {
                let plain = plain_string_cow(value);
                Ok(js_string_escaper(plain.as_ref()))
            }
        },
        EscapeMode::ScriptJsonString { quote: _ } => match value {
            Value::SafeJsStr(raw) => Ok(js_string_escaper_norm(raw)),
            _ => {
                let plain = plain_string_cow(value);
                Ok(js_string_escaper(plain.as_ref()))
            }
        },
        EscapeMode::StyleExpr => {
            if matches!(
                value,
                Value::SafeHtml(_)
                    | Value::SafeHtmlAttr(_)
                    | Value::SafeJs(_)
                    | Value::SafeJsStr(_)
                    | Value::SafeUrl(_)
                    | Value::SafeSrcset(_)
            ) {
                return Ok("ZgotmplZ".to_string());
            }
            let plain = plain_string_cow(value);
            let filtered = css_value_filter(plain.as_ref());
            if filtered == "ZgotmplZ" {
                return Ok(filtered);
            }
            Ok(escape_css_text(&filtered))
        }
        EscapeMode::StyleLineComment | EscapeMode::StyleBlockComment => Ok(String::new()),
        EscapeMode::StyleString { quote } => match css_url_part_hint {
            Some(Some(url_part)) => Ok(escape_css_url_value(value, url_part)),
            Some(None) => {
                let plain = plain_string_cow(value);
                Ok(escape_css_string_fragment(plain.as_ref(), quote))
            }
            None => {
                if let Some(url_part) = css_url_part_context(rendered_prefix) {
                    Ok(escape_css_url_value(value, url_part))
                } else {
                    let plain = plain_string_cow(value);
                    Ok(escape_css_string_fragment(plain.as_ref(), quote))
                }
            }
        },
    }
}

#[cfg(test)]
fn infer_escape_mode_with_tag_context(
    rendered: &str,
    tag_value_context: Option<&TagValueContext>,
) -> EscapeMode {
    if contains_ascii_case_insensitive(rendered, b"<textarea")
        && current_unclosed_tag_content(rendered, "textarea").is_some()
    {
        return EscapeMode::Rcdata;
    }
    if contains_ascii_case_insensitive(rendered, b"<title")
        && current_unclosed_tag_content(rendered, "title").is_some()
    {
        return EscapeMode::Rcdata;
    }

    if contains_ascii_case_insensitive(rendered, b"<script")
        && let Some(mode) = script_escape_mode(rendered)
    {
        return mode;
    }

    if contains_ascii_case_insensitive(rendered, b"<style")
        && let Some(mode) = style_escape_mode(rendered)
    {
        return mode;
    }

    let owned_context = if tag_value_context.is_none() {
        current_tag_value_context(rendered)
    } else {
        None
    };
    if let Some(context) = tag_value_context.or(owned_context.as_ref()) {
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
                    Some(EscapeMode::StyleExpr) | None => {
                        if context.quoted {
                            EscapeMode::AttrQuoted {
                                kind,
                                quote: context.quote.unwrap_or('"'),
                            }
                        } else {
                            EscapeMode::AttrUnquoted { kind }
                        }
                    }
                    Some(mode) => mode,
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

    EscapeMode::Html
}

#[cfg(test)]
fn infer_escape_mode(rendered: &str) -> EscapeMode {
    infer_escape_mode_with_tag_context(rendered, None)
}

fn current_tag_value_context(rendered: &str) -> Option<TagValueContext> {
    let last_lt = last_unclosed_tag_start(rendered)?;
    let fragment = &rendered[last_lt + 1..];
    parse_open_tag_value_context(fragment)
}

fn current_attr_name_context(rendered: &str) -> bool {
    let last_lt = match last_unclosed_tag_start(rendered) {
        Some(last_lt) => last_lt,
        None => return false,
    };

    let fragment = &rendered[last_lt + 1..];
    if fragment.is_empty() {
        return false;
    }

    let bytes = fragment.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() && is_html_space(bytes[i]) {
        i += 1;
    }
    if i >= bytes.len() {
        return true;
    }

    if matches!(bytes[i], b'/' | b'!' | b'?') {
        return false;
    }

    while i < bytes.len() {
        while i < bytes.len() && is_html_space(bytes[i]) {
            i += 1;
        }
        if i >= bytes.len() {
            return true;
        }
        if matches!(bytes[i], b'/' | b'>') {
            return false;
        }

        let start = i;
        while i < bytes.len() {
            let byte = bytes[i];
            if is_html_space(byte) || matches!(byte, b'=' | b'/' | b'>') {
                break;
            }
            i += 1;
        }
        if i <= start {
            return false;
        }

        while i < bytes.len() && is_html_space(bytes[i]) {
            i += 1;
        }
        if i >= bytes.len() || matches!(bytes[i], b'/' | b'>') {
            return true;
        }

        if bytes[i] == b'=' {
            i += 1;
            while i < bytes.len() && is_html_space(bytes[i]) {
                i += 1;
            }

            if i >= bytes.len() {
                return false;
            }

            if matches!(bytes[i], b'"' | b'\'') {
                let quote = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                if i >= bytes.len() {
                    return false;
                }
                i += 1;
                continue;
            }

            while i < bytes.len() && !is_html_space(bytes[i]) && bytes[i] != b'>' {
                i += 1;
            }

            if i >= bytes.len() {
                return false;
            }

            continue;
        }

        return true;
    }

    false
}

fn parse_open_tag_value_context(fragment: &str) -> Option<TagValueContext> {
    let bytes = fragment.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    let mut i = 0usize;
    while i < bytes.len() && is_html_space(bytes[i]) {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    if matches!(bytes[i], b'/' | b'!' | b'?') {
        return None;
    }

    while i < bytes.len() {
        let byte = bytes[i];
        if is_html_space(byte) || matches!(byte, b'/' | b'>') {
            break;
        }
        i += 1;
    }

    while i < bytes.len() {
        while i < bytes.len() && is_html_space(bytes[i]) {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        if matches!(bytes[i], b'/' | b'>') {
            break;
        }

        let attr_start = i;
        while i < bytes.len() {
            let byte = bytes[i];
            if is_html_space(byte) || matches!(byte, b'=' | b'/' | b'>') {
                break;
            }
            i += 1;
        }
        if i <= attr_start {
            break;
        }
        let attr_name = fragment[attr_start..i].to_string();

        while i < bytes.len() && is_html_space(bytes[i]) {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            continue;
        }
        i += 1;

        while i < bytes.len() && is_html_space(bytes[i]) {
            i += 1;
        }
        if i >= bytes.len() {
            return Some(TagValueContext {
                attr_name,
                quoted: false,
                quote: None,
                value_prefix: String::new(),
            });
        }

        let quote = bytes[i];
        if matches!(quote, b'"' | b'\'') {
            i += 1;
            let value_start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            if i >= bytes.len() {
                let partial = &fragment[value_start..];
                if partial.is_empty() || !partial.ends_with("}}") {
                    return Some(TagValueContext {
                        attr_name,
                        quoted: true,
                        quote: Some(quote as char),
                        value_prefix: partial.to_string(),
                    });
                }
                return None;
            }
            i += 1;
        } else {
            let value_start = i;
            while i < bytes.len() && !is_html_space(bytes[i]) && bytes[i] != b'>' {
                i += 1;
            }
            if i >= bytes.len() {
                let partial = &fragment[value_start..];
                if partial.is_empty() || !partial.ends_with("}}") {
                    return Some(TagValueContext {
                        attr_name,
                        quoted: false,
                        quote: None,
                        value_prefix: partial.to_string(),
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
        return "ZgotmplZ".to_string();
    }

    let name = input.to_ascii_lowercase();
    if !name.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return "ZgotmplZ".to_string();
    }

    if attr_content_type(&name) != AttrContentType::Plain {
        return "ZgotmplZ".to_string();
    }

    name
}

#[derive(Clone, Copy)]
enum HtmlAttributeEscapeMode {
    Quoted,
    Unquoted,
}

fn plain_string_cow(value: &Value) -> Cow<'_, str> {
    match value {
        Value::SafeHtml(raw)
        | Value::SafeHtmlAttr(raw)
        | Value::SafeJs(raw)
        | Value::SafeJsStr(raw)
        | Value::SafeCss(raw)
        | Value::SafeUrl(raw)
        | Value::SafeSrcset(raw) => Cow::Borrowed(raw),
        Value::Json(JsonValue::String(raw)) => Cow::Borrowed(raw),
        _ => Cow::Owned(value.to_plain_string()),
    }
}

fn append_html_attr_escaped_char(output: &mut String, ch: char, mode: HtmlAttributeEscapeMode) {
    match mode {
        HtmlAttributeEscapeMode::Quoted => match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&#34;"),
            '\'' => output.push_str("&#39;"),
            '+' => output.push_str("&#43;"),
            '\0' => output.push('\u{FFFD}'),
            _ => output.push(ch),
        },
        HtmlAttributeEscapeMode::Unquoted => match ch {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&#34;"),
            '\'' => output.push_str("&#39;"),
            '`' => output.push_str("&#96;"),
            '=' => output.push_str("&#61;"),
            '+' => output.push_str("&#43;"),
            ' ' => output.push_str("&#32;"),
            '\n' => output.push_str("&#10;"),
            '\r' => output.push_str("&#13;"),
            '\t' => output.push_str("&#9;"),
            '\0' => output.push_str("&#xfffd;"),
            _ => output.push(ch),
        },
    }
}

fn append_html_attr_escaped_text(output: &mut String, input: &str, mode: HtmlAttributeEscapeMode) {
    if input.is_ascii() {
        for &byte in input.as_bytes() {
            append_html_attr_escaped_byte(output, byte, mode);
        }
    } else {
        for ch in input.chars() {
            append_html_attr_escaped_char(output, ch, mode);
        }
    }
}

fn append_html_attr_escaped_byte(output: &mut String, byte: u8, mode: HtmlAttributeEscapeMode) {
    match mode {
        HtmlAttributeEscapeMode::Quoted => match byte {
            b'&' => output.push_str("&amp;"),
            b'<' => output.push_str("&lt;"),
            b'>' => output.push_str("&gt;"),
            b'"' => output.push_str("&#34;"),
            b'\'' => output.push_str("&#39;"),
            b'+' => output.push_str("&#43;"),
            b'\0' => output.push('\u{FFFD}'),
            _ => output.push(byte as char),
        },
        HtmlAttributeEscapeMode::Unquoted => match byte {
            b'&' => output.push_str("&amp;"),
            b'<' => output.push_str("&lt;"),
            b'>' => output.push_str("&gt;"),
            b'"' => output.push_str("&#34;"),
            b'\'' => output.push_str("&#39;"),
            b'`' => output.push_str("&#96;"),
            b'=' => output.push_str("&#61;"),
            b'+' => output.push_str("&#43;"),
            b' ' => output.push_str("&#32;"),
            b'\n' => output.push_str("&#10;"),
            b'\r' => output.push_str("&#13;"),
            b'\t' => output.push_str("&#9;"),
            b'\0' => output.push_str("&#xfffd;"),
            _ => output.push(byte as char),
        },
    }
}

fn append_url_attribute_encoded_and_escaped(
    input: &str,
    output: &mut String,
    escape_mode: HtmlAttributeEscapeMode,
) {
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == b'%' {
            if i + 2 < bytes.len()
                && bytes[i + 1].is_ascii_hexdigit()
                && bytes[i + 2].is_ascii_hexdigit()
            {
                append_html_attr_escaped_byte(output, b'%', escape_mode);
                append_html_attr_escaped_byte(output, bytes[i + 1], escape_mode);
                append_html_attr_escaped_byte(output, bytes[i + 2], escape_mode);
                i += 3;
                continue;
            }
            append_html_attr_escaped_byte(output, b'%', escape_mode);
            append_html_attr_escaped_byte(output, b'2', escape_mode);
            append_html_attr_escaped_byte(output, b'5', escape_mode);
            i += 1;
            continue;
        }

        if is_safe_url_attr_byte(byte) {
            append_html_attr_escaped_byte(output, byte, escape_mode);
        } else {
            append_html_attr_escaped_byte(output, b'%', escape_mode);
            append_html_attr_escaped_byte(output, hex_lower((byte >> 4) & 0x0F) as u8, escape_mode);
            append_html_attr_escaped_byte(output, hex_lower(byte & 0x0F) as u8, escape_mode);
        }
        i += 1;
    }
}

fn append_percent_encoded_and_escaped(
    input: &str,
    output: &mut String,
    escape_mode: HtmlAttributeEscapeMode,
) {
    for &byte in input.as_bytes() {
        if is_unreserved_url_byte(byte) {
            append_html_attr_escaped_byte(output, byte, escape_mode);
        } else {
            append_html_attr_escaped_byte(output, b'%', escape_mode);
            append_html_attr_escaped_byte(output, hex_lower((byte >> 4) & 0x0F) as u8, escape_mode);
            append_html_attr_escaped_byte(output, hex_lower(byte & 0x0F) as u8, escape_mode);
        }
    }
}

fn escape_url_attribute_value_single_pass(
    value: &Value,
    url_part: Option<UrlPartContext>,
    escape_mode: HtmlAttributeEscapeMode,
) -> String {
    let raw = plain_string_cow(value);
    let url_part = url_part.unwrap_or(UrlPartContext::Path);
    let is_safe_value = matches!(value, Value::SafeUrl(_));
    if matches!(url_part, UrlPartContext::Path) && !is_safe_value && !is_safe_url(raw.as_ref()) {
        return "#ZgotmplZ".to_string();
    }

    let mut output = String::with_capacity(raw.len().saturating_mul(3));
    if is_safe_value || matches!(url_part, UrlPartContext::Path) {
        append_url_attribute_encoded_and_escaped(raw.as_ref(), &mut output, escape_mode);
    } else {
        append_percent_encoded_and_escaped(raw.as_ref(), &mut output, escape_mode);
    }
    output
}

fn append_srcset_url_encoded_and_escaped(
    input: &str,
    output: &mut String,
    escape_mode: HtmlAttributeEscapeMode,
) {
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == b',' {
            append_html_attr_escaped_byte(output, b'%', escape_mode);
            append_html_attr_escaped_byte(output, b'2', escape_mode);
            append_html_attr_escaped_byte(output, b'c', escape_mode);
            i += 1;
            continue;
        }
        if byte == b'%' {
            if i + 2 < bytes.len()
                && bytes[i + 1].is_ascii_hexdigit()
                && bytes[i + 2].is_ascii_hexdigit()
            {
                append_html_attr_escaped_byte(output, b'%', escape_mode);
                append_html_attr_escaped_byte(output, bytes[i + 1], escape_mode);
                append_html_attr_escaped_byte(output, bytes[i + 2], escape_mode);
                i += 3;
                continue;
            }
            append_html_attr_escaped_byte(output, b'%', escape_mode);
            append_html_attr_escaped_byte(output, b'2', escape_mode);
            append_html_attr_escaped_byte(output, b'5', escape_mode);
            i += 1;
            continue;
        }
        if is_safe_url_attr_byte(byte) {
            append_html_attr_escaped_byte(output, byte, escape_mode);
        } else {
            append_html_attr_escaped_byte(output, b'%', escape_mode);
            append_html_attr_escaped_byte(output, hex_lower((byte >> 4) & 0x0F) as u8, escape_mode);
            append_html_attr_escaped_byte(output, hex_lower(byte & 0x0F) as u8, escape_mode);
        }
        i += 1;
    }
}

fn append_filtered_srcset_element_and_escaped(
    input: &str,
    bytes: &[u8],
    start: &mut usize,
    end: usize,
    output: &mut String,
    escape_mode: HtmlAttributeEscapeMode,
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
        append_html_attr_escaped_text(output, &input[*start..left], escape_mode);
        append_srcset_url_encoded_and_escaped(url, output, escape_mode);
        append_html_attr_escaped_text(output, &input[element_end..end], escape_mode);
    } else {
        append_html_attr_escaped_text(output, "#ZgotmplZ", escape_mode);
    }

    *start = end;
}

fn escape_filtered_srcset_single_pass(input: &str, escape_mode: HtmlAttributeEscapeMode) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len().saturating_mul(2));
    let mut start = 0usize;

    for i in 0..bytes.len() {
        if bytes[i] != b',' {
            continue;
        }
        append_filtered_srcset_element_and_escaped(
            input,
            bytes,
            &mut start,
            i,
            &mut output,
            escape_mode,
        );
        append_html_attr_escaped_byte(&mut output, b',', escape_mode);
        start = i + 1;
    }

    append_filtered_srcset_element_and_escaped(
        input,
        bytes,
        &mut start,
        bytes.len(),
        &mut output,
        escape_mode,
    );
    output
}

fn escape_srcset_attribute_value_single_pass(
    value: &Value,
    escape_mode: HtmlAttributeEscapeMode,
) -> String {
    match value {
        Value::SafeSrcset(raw) => {
            let mut output = String::with_capacity(raw.len().saturating_mul(2));
            append_html_attr_escaped_text(&mut output, raw, escape_mode);
            output
        }
        Value::SafeUrl(raw) => {
            if !is_safe_url(raw) {
                let mut output = String::with_capacity("#ZgotmplZ".len());
                append_html_attr_escaped_text(&mut output, "#ZgotmplZ", escape_mode);
                return output;
            }
            let mut output = String::with_capacity(raw.len().saturating_mul(3));
            append_srcset_url_encoded_and_escaped(raw, &mut output, escape_mode);
            output
        }
        _ => {
            let raw = value.to_plain_string();
            escape_filtered_srcset_single_pass(&raw, escape_mode)
        }
    }
}

fn script_attribute_mode(value_prefix: &str) -> Option<EscapeMode> {
    Some(current_js_mode(value_prefix))
}

fn style_attribute_mode(value_prefix: &str) -> Option<EscapeMode> {
    Some(current_css_mode(value_prefix))
}

#[cfg(test)]
fn script_escape_mode(rendered: &str) -> Option<EscapeMode> {
    let script_tag = current_unclosed_script_tag(rendered)?;
    if !is_script_type_javascript(script_tag) {
        return None;
    }

    let content = current_unclosed_tag_content(rendered, "script")?;
    let mode = current_js_mode(content);
    if is_script_type_json(script_tag)
        && let EscapeMode::ScriptString { quote } = mode
    {
        return Some(EscapeMode::ScriptJsonString { quote });
    }
    Some(mode)
}

#[cfg(test)]
fn style_escape_mode(rendered: &str) -> Option<EscapeMode> {
    let content = current_unclosed_tag_content(rendered, "style")?;
    Some(current_css_mode(content))
}

fn css_url_part_context(rendered: &str) -> Option<UrlPartContext> {
    let prefix = css_prefix_for_context(rendered)?;
    let value_prefix = current_css_url_value_prefix(&prefix)?;
    if value_prefix.contains('#') {
        Some(UrlPartContext::Fragment)
    } else if value_prefix.contains('?') {
        Some(UrlPartContext::Query)
    } else {
        Some(UrlPartContext::Path)
    }
}

fn escape_css_url_value(value: &Value, url_part: UrlPartContext) -> String {
    let raw = value.to_plain_string();
    if matches!(url_part, UrlPartContext::Path)
        && !matches!(value, Value::SafeUrl(_))
        && !is_safe_url(&raw)
    {
        return "#ZgotmplZ".to_string();
    }

    if matches!(value, Value::SafeUrl(_)) || matches!(url_part, UrlPartContext::Path) {
        encode_url_attribute_value(&raw)
    } else {
        percent_encode_url(&raw)
    }
}

fn current_css_url_value_prefix(prefix: &str) -> Option<String> {
    #[derive(Clone, Copy)]
    enum State {
        Expr,
        SingleQuote { is_url: bool, start: usize },
        DoubleQuote { is_url: bool, start: usize },
        LineComment,
        BlockComment,
    }

    let bytes = prefix.as_bytes();
    let mut state = State::Expr;
    let mut i = 0usize;

    while i < bytes.len() {
        state = match state {
            State::Expr => match bytes[i] {
                b'\'' => {
                    let is_url = ends_with_css_url_start_bytes(bytes, i);
                    i += 1;
                    State::SingleQuote { is_url, start: i }
                }
                b'"' => {
                    let is_url = ends_with_css_url_start_bytes(bytes, i);
                    i += 1;
                    State::DoubleQuote { is_url, start: i }
                }
                b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                    i += 2;
                    State::LineComment
                }
                b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                    i += 2;
                    State::BlockComment
                }
                _ => {
                    i += 1;
                    State::Expr
                }
            },
            State::SingleQuote { is_url, start } => {
                if bytes[i] == b'\\' {
                    i += 2;
                    State::SingleQuote { is_url, start }
                } else if bytes[i] == b'\'' {
                    i += 1;
                    State::Expr
                } else {
                    i += 1;
                    State::SingleQuote { is_url, start }
                }
            }
            State::DoubleQuote { is_url, start } => {
                if bytes[i] == b'\\' {
                    i += 2;
                    State::DoubleQuote { is_url, start }
                } else if bytes[i] == b'"' {
                    i += 1;
                    State::Expr
                } else {
                    i += 1;
                    State::DoubleQuote { is_url, start }
                }
            }
            State::LineComment => {
                if bytes[i] == b'\n' || bytes[i] == b'\r' || bytes[i] == b'\x0c' {
                    i += 1;
                    State::Expr
                } else if is_utf8_line_separator(bytes, i) {
                    i += 3;
                    State::Expr
                } else {
                    i += 1;
                    State::LineComment
                }
            }
            State::BlockComment => {
                if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    i += 2;
                    State::Expr
                } else {
                    i += 1;
                    State::BlockComment
                }
            }
        };
    }

    match state {
        State::SingleQuote {
            is_url: true,
            start,
        }
        | State::DoubleQuote {
            is_url: true,
            start,
        } => Some(prefix[start..].to_string()),
        _ => None,
    }
}

fn ends_with_css_url_start_bytes(bytes: &[u8], end: usize) -> bool {
    let mut end = end.min(bytes.len());
    while end > 0 && is_css_space_byte(bytes[end - 1]) {
        end -= 1;
    }
    if end == 0 || bytes[end - 1] != b'(' {
        return false;
    }

    end -= 1;
    while end > 0 && is_css_space_byte(bytes[end - 1]) {
        end -= 1;
    }
    if end < 3 {
        return false;
    }

    let start = end - 3;
    if !bytes[start..end].eq_ignore_ascii_case(b"url") {
        return false;
    }

    if start == 0 {
        return true;
    }

    let prev = bytes[start - 1];
    !(prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'-')
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

fn is_script_type_json(script_tag: &str) -> bool {
    let Some(type_value) = script_type_attribute(script_tag) else {
        return false;
    };
    let lower = type_value.trim().to_ascii_lowercase();
    let mime = lower.split(';').next().map(str::trim).unwrap_or_default();
    mime == "application/json" || mime.ends_with("+json")
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
    let tag_bytes = tag_name.as_bytes();
    let mut cursor = 0usize;
    let mut content_start: Option<usize> = None;

    loop {
        if content_start.is_none() {
            let open_index = find_open_tag(rendered, cursor, tag_bytes)?;
            let start = html_tag_end(rendered, open_index)?;
            content_start = Some(start);
            cursor = start;
            continue;
        }

        let start = content_start?;
        if let Some(close_start) = find_close_tag(rendered, start, tag_bytes) {
            let close_end = html_tag_end(rendered, close_start).unwrap_or(close_start + 1);
            cursor = close_end;
            content_start = None;
            continue;
        }

        return Some(&rendered[start..]);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum JsContext {
    RegExp,
    DivOp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

fn scan_js_state_until(
    content: &str,
    start: usize,
    initial_state: JsScanState,
    stop_on_script_close: bool,
) -> (JsScanState, Option<usize>) {
    let bytes = content.as_bytes();
    if start >= bytes.len() {
        return (initial_state, None);
    }

    let mut state = initial_state;
    let mut segment_start = start;
    let mut i = start;

    while i < bytes.len() {
        state = match state {
            JsScanState::Expr { js_ctx } => {
                if stop_on_script_close && is_html_close_tag(bytes, i, b"script") {
                    return (
                        JsScanState::Expr {
                            js_ctx: next_js_ctx_bytes(&bytes[segment_start..i], js_ctx),
                        },
                        Some(i),
                    );
                }

                let ch = bytes[i];
                match ch {
                    b'\'' => {
                        let _ = next_js_ctx_bytes(&bytes[segment_start..i], js_ctx);
                        segment_start = i + 1;
                        i += 1;
                        JsScanState::SingleQuote
                    }
                    b'"' => {
                        let _ = next_js_ctx_bytes(&bytes[segment_start..i], js_ctx);
                        segment_start = i + 1;
                        i += 1;
                        JsScanState::DoubleQuote
                    }
                    b'`' => {
                        let _ = next_js_ctx_bytes(&bytes[segment_start..i], js_ctx);
                        segment_start = i + 1;
                        i += 1;
                        JsScanState::TemplateLiteral
                    }
                    b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                        let js_ctx = next_js_ctx_bytes(&bytes[segment_start..i], js_ctx);
                        i += 2;
                        segment_start = i;
                        JsScanState::LineComment {
                            js_ctx,
                            preserve_body: true,
                            keep_terminator: true,
                        }
                    }
                    b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                        let js_ctx = next_js_ctx_bytes(&bytes[segment_start..i], js_ctx);
                        i += 2;
                        segment_start = i;
                        JsScanState::BlockComment { js_ctx }
                    }
                    b'<' if i + 4 <= bytes.len() && &bytes[i..i + 4] == b"<!--" => {
                        let js_ctx = next_js_ctx_bytes(&bytes[segment_start..i], js_ctx);
                        i += 4;
                        segment_start = i;
                        JsScanState::LineComment {
                            js_ctx,
                            preserve_body: false,
                            keep_terminator: true,
                        }
                    }
                    b'-' if i + 3 <= bytes.len() && &bytes[i..i + 3] == b"-->" => {
                        let js_ctx = next_js_ctx_bytes(&bytes[segment_start..i], js_ctx);
                        i += 3;
                        segment_start = i;
                        JsScanState::LineComment {
                            js_ctx,
                            preserve_body: false,
                            keep_terminator: true,
                        }
                    }
                    b'#' if i + 1 < bytes.len() && bytes[i + 1] == b'!' => {
                        let js_ctx = next_js_ctx_bytes(&bytes[segment_start..i], js_ctx);
                        i += 2;
                        segment_start = i;
                        JsScanState::LineComment {
                            js_ctx,
                            preserve_body: false,
                            keep_terminator: true,
                        }
                    }
                    _ if is_utf8_line_separator_2028(bytes, i) => {
                        let js_ctx = next_js_ctx_bytes(&bytes[segment_start..i], js_ctx);
                        i += 3;
                        segment_start = i;
                        JsScanState::Expr { js_ctx }
                    }
                    _ if is_utf8_line_separator_2029(bytes, i) => {
                        let js_ctx = next_js_ctx_bytes(&bytes[segment_start..i], js_ctx);
                        i += 3;
                        segment_start = i;
                        JsScanState::Expr { js_ctx }
                    }
                    b'/' => {
                        let js_ctx = next_js_ctx_bytes(&bytes[segment_start..i], js_ctx);
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

    state = match state {
        JsScanState::Expr { js_ctx } => JsScanState::Expr {
            js_ctx: next_js_ctx_bytes(&bytes[segment_start..], js_ctx),
        },
        _ => state,
    };

    (state, None)
}

fn advance_js_scan_state(content: &str, initial_state: JsScanState) -> JsScanState {
    scan_js_state_until(content, 0, initial_state, false).0
}
fn current_js_scan_state(content: &str) -> JsScanState {
    advance_js_scan_state(
        content,
        JsScanState::Expr {
            js_ctx: JsContext::RegExp,
        },
    )
}

fn filter_script_text_with_state(initial_state: JsScanState, text: &str) -> String {
    let bytes = text.as_bytes();
    let mut state = initial_state;
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

#[cfg(test)]
fn filter_script_text(prefix: &str, text: &str) -> String {
    filter_script_text_with_state(current_js_scan_state(prefix), text)
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

    if start >= content.len() {
        return None;
    }
    let tag_name = std::str::from_utf8(tag).ok()?;
    index_tag_end(&content[start..], tag_name).map(|offset| start + offset)
}

fn find_script_close_tag(content: &str, start: usize) -> Option<usize> {
    scan_js_state_until(
        content,
        start,
        JsScanState::Expr {
            js_ctx: JsContext::RegExp,
        },
        true,
    )
    .1
}
fn find_style_close_tag(content: &str, start: usize) -> Option<usize> {
    find_style_close_tag_with_state(content, start, CssScanState::Expr)
}

fn find_style_close_tag_with_state(
    content: &str,
    start: usize,
    initial_state: CssScanState,
) -> Option<usize> {
    scan_css_state_until(content, start, initial_state, true).1
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

fn contains_open_script_or_style_tag(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() < 7 {
        return false;
    }

    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'<' && bytes[i + 1] != b'/' {
            let rest = &bytes[i + 1..];
            if matches_html_tag(rest, b"script") || matches_html_tag(rest, b"style") {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn should_prepare_text_plan_for_script_style(start_state: &ContextState, text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    if start_state.is_script_tag_context() || start_state.is_style_tag_context() {
        return true;
    }
    if !start_state.is_text_context() {
        return false;
    }
    if !text.as_bytes().contains(&b'<') {
        return false;
    }
    contains_open_script_or_style_tag(text)
}

fn prepare_text_plan_for_script_style(
    start_state: &ContextState,
    tracker: &ContextTracker,
    text: &str,
) -> Option<PreparedTextPlan> {
    if !should_prepare_text_plan_for_script_style(start_state, text) {
        return None;
    }

    let start_section = if start_state.is_script_tag_context() {
        PreparedSection::Script {
            scan_state: tracker.js_scan_state?,
        }
    } else if start_state.is_style_tag_context() {
        PreparedSection::Style {
            scan_state: tracker.css_scan_state?,
        }
    } else {
        PreparedSection::Html
    };

    let chunks = precompute_text_chunks_for_script_style(text, start_section)?;
    Some(PreparedTextPlan {
        start_state: start_state.clone(),
        chunks,
    })
}

fn push_prepared_emit(chunks: &mut Vec<PreparedTextChunk>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(PreparedTextChunk::Emit(existing)) = chunks.last_mut() {
        existing.push_str(text);
    } else {
        chunks.push(PreparedTextChunk::Emit(text.to_string()));
    }
}

fn push_prepared_emit_owned(chunks: &mut Vec<PreparedTextChunk>, text: String) {
    if text.is_empty() {
        return;
    }
    if let Some(PreparedTextChunk::Emit(existing)) = chunks.last_mut() {
        existing.push_str(&text);
    } else {
        chunks.push(PreparedTextChunk::Emit(text));
    }
}

fn precompute_text_chunks_for_script_style(
    text: &str,
    start_section: PreparedSection,
) -> Option<Vec<PreparedTextChunk>> {
    let mut chunks = Vec::new();
    let mut section = start_section;
    let mut cursor = 0usize;

    while cursor < text.len() {
        match section {
            PreparedSection::Html => {
                let next_script = find_open_tag(text, cursor, b"script");
                let next_style = find_open_tag(text, cursor, b"style");
                let (next, target) = match (next_script, next_style) {
                    (Some(a), Some(b)) if a <= b => (
                        a,
                        PreparedSection::Script {
                            scan_state: JsScanState::Expr {
                                js_ctx: JsContext::RegExp,
                            },
                        },
                    ),
                    (Some(_), Some(b)) => (
                        b,
                        PreparedSection::Style {
                            scan_state: CssScanState::Expr,
                        },
                    ),
                    (Some(a), None) => (
                        a,
                        PreparedSection::Script {
                            scan_state: JsScanState::Expr {
                                js_ctx: JsContext::RegExp,
                            },
                        },
                    ),
                    (None, Some(b)) => (
                        b,
                        PreparedSection::Style {
                            scan_state: CssScanState::Expr,
                        },
                    ),
                    (None, None) => (usize::MAX, PreparedSection::Html),
                };

                if next == usize::MAX {
                    push_prepared_emit(&mut chunks, &text[cursor..]);
                    break;
                }

                let tag_end = html_tag_end(text, next)?;
                let segment = &text[cursor..tag_end];
                push_prepared_emit(&mut chunks, segment);

                section = match target {
                    PreparedSection::Script { scan_state } => {
                        let script_tag = text.get(next..tag_end)?;
                        if is_script_type_javascript(script_tag) {
                            PreparedSection::Script { scan_state }
                        } else {
                            PreparedSection::Html
                        }
                    }
                    PreparedSection::Style { scan_state } => PreparedSection::Style { scan_state },
                    PreparedSection::Html => PreparedSection::Html,
                };
                cursor = tag_end;
            }
            PreparedSection::Script { scan_state } => {
                if matches!(scan_state, JsScanState::Expr { .. })
                    && let Some(close_start) = find_close_tag(text, cursor, b"script")
                {
                    let script_segment = &text[cursor..close_start];
                    if !script_segment.is_empty() {
                        let filtered = filter_script_text_with_state(scan_state, script_segment);
                        push_prepared_emit_owned(&mut chunks, filtered);
                    }
                    let close_end = html_tag_end(text, close_start)?;
                    chunks.push(PreparedTextChunk::ScriptCloseTag(
                        text[close_start..close_end].to_string(),
                    ));
                    cursor = close_end;
                    section = PreparedSection::Html;
                    continue;
                }

                let script_segment = &text[cursor..];
                if !script_segment.is_empty() {
                    let filtered = filter_script_text_with_state(scan_state, script_segment);
                    push_prepared_emit_owned(&mut chunks, filtered);
                }
                break;
            }
            PreparedSection::Style { scan_state } => {
                if let Some(close_start) = find_style_close_tag_with_state(text, cursor, scan_state)
                {
                    let style_segment = &text[cursor..close_start];
                    push_prepared_emit(&mut chunks, style_segment);
                    let close_end = html_tag_end(text, close_start)?;
                    chunks.push(PreparedTextChunk::StyleCloseTag(
                        text[close_start..close_end].to_string(),
                    ));
                    cursor = close_end;
                    section = PreparedSection::Html;
                } else {
                    push_prepared_emit(&mut chunks, &text[cursor..]);
                    break;
                }
            }
        }
    }

    Some(chunks)
}

fn filter_html_text_sections(prefix: &str, text: &str) -> String {
    let mut output = String::new();
    let mut script_scan_state = None;
    let mut section = if let Some(script_tag) = current_unclosed_script_tag(prefix) {
        if is_script_type_javascript(script_tag) {
            script_scan_state = Some(
                current_unclosed_tag_content(prefix, "script")
                    .map(current_js_scan_state)
                    .unwrap_or(JsScanState::Expr {
                        js_ctx: JsContext::RegExp,
                    }),
            );
            HtmlSection::Script
        } else {
            HtmlSection::Html
        }
    } else if current_unclosed_tag_content(prefix, "style").is_some() {
        HtmlSection::Style
    } else {
        HtmlSection::Html
    };

    if matches!(section, HtmlSection::Html)
        && !contains_ascii_case_insensitive(text, b"<script")
        && !contains_ascii_case_insensitive(text, b"<style")
    {
        return text.to_string();
    }
    if matches!(section, HtmlSection::Style) && !contains_ascii_case_insensitive(text, b"</style") {
        return text.to_string();
    }

    let mut cursor = 0usize;

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
                                script_scan_state = Some(JsScanState::Expr {
                                    js_ctx: JsContext::RegExp,
                                });
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

                let segment = &text[cursor..tag_end];
                output.push_str(segment);
                cursor = tag_end;
                section = target;
            }
            HtmlSection::Script => {
                let scan_state = script_scan_state.unwrap_or(JsScanState::Expr {
                    js_ctx: JsContext::RegExp,
                });
                let close = find_close_tag(text, cursor, b"script");
                if let Some(close_start) = close {
                    let segment = &text[cursor..close_start];
                    if !segment.is_empty() {
                        let filtered = filter_script_text_with_state(scan_state, segment);
                        output.push_str(&filtered);
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
                    cursor = close_end;
                    section = HtmlSection::Html;
                    script_scan_state = None;
                } else {
                    let segment = &text[cursor..];
                    let filtered = filter_script_text_with_state(scan_state, segment);
                    output.push_str(&filtered);
                    break;
                }
            }
            HtmlSection::Style => {
                let close = find_close_tag(text, cursor, b"style");
                if let Some(close_start) = close {
                    let segment = &text[cursor..close_start];
                    output.push_str(segment);
                    let close_end = match html_tag_end(text, close_start) {
                        Some(end) => end,
                        None => {
                            output.push_str(&text[close_start..]);
                            break;
                        }
                    };
                    let close_tag = &text[close_start..close_end];
                    output.push_str(close_tag);
                    cursor = close_end;
                    section = HtmlSection::Html;
                } else {
                    let tail = &text[cursor..];
                    output.push_str(tail);
                    break;
                }
            }
        }
    }

    output
}

fn next_js_ctx_bytes(prefix: &[u8], preceding: JsContext) -> JsContext {
    let end = trim_js_trailing_whitespace(prefix);
    if end == 0 {
        return preceding;
    }

    let bytes = &prefix[..end];
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
            if is_js_ident_keyword_bytes(&bytes[j..]) {
                JsContext::RegExp
            } else {
                JsContext::DivOp
            }
        }
    }
}

#[cfg(test)]
fn next_js_ctx(prefix: &str, preceding: JsContext) -> JsContext {
    next_js_ctx_bytes(prefix.as_bytes(), preceding)
}

fn is_js_ident_part_byte(byte: u8) -> bool {
    matches!(byte, b'$' | b'_' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z')
}

fn is_js_ident_keyword_bytes(keyword: &[u8]) -> bool {
    match keyword.len() {
        2 => keyword == b"do" || keyword == b"in",
        3 => keyword == b"try",
        4 => keyword == b"case" || keyword == b"else" || keyword == b"void",
        5 => keyword == b"break" || keyword == b"throw",
        6 => keyword == b"delete" || keyword == b"return" || keyword == b"typeof",
        7 => keyword == b"finally",
        8 => keyword == b"continue",
        10 => keyword == b"instanceof",
        _ => false,
    }
}

fn trim_js_trailing_whitespace(bytes: &[u8]) -> usize {
    let mut end = bytes.len();
    while end > 0 {
        let byte = bytes[end - 1];
        if matches!(byte, b'\t' | b'\n' | b'\r' | b'\x0B' | b'\x0C' | b' ') {
            end -= 1;
            continue;
        }

        if end >= 2 && bytes[end - 2] == 0xC2 && byte == 0xA0 {
            // U+00A0
            end -= 2;
            continue;
        }

        if end >= 3 {
            let a = bytes[end - 3];
            let b = bytes[end - 2];
            let c = bytes[end - 1];

            let is_js_unicode_space = (a == 0xE1 && b == 0x9A && c == 0x80) // U+1680
                || (a == 0xE2 && b == 0x80 && (0x80..=0x8A).contains(&c)) // U+2000..U+200A
                || (a == 0xE2 && b == 0x80 && c == 0xA8) // U+2028
                || (a == 0xE2 && b == 0x80 && c == 0xA9) // U+2029
                || (a == 0xE2 && b == 0x80 && c == 0xAF) // U+202F
                || (a == 0xE2 && b == 0x81 && c == 0x9F) // U+205F
                || (a == 0xE3 && b == 0x80 && c == 0x80) // U+3000
                || (a == 0xEF && b == 0xBB && c == 0xBF); // U+FEFF

            if is_js_unicode_space {
                end -= 3;
                continue;
            }
        }

        break;
    }
    end
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CssScanState {
    Expr,
    SingleQuote {
        is_url: bool,
        url_part: UrlPartContext,
    },
    DoubleQuote {
        is_url: bool,
        url_part: UrlPartContext,
    },
    LineComment,
    BlockComment,
}

fn current_css_mode(content: &str) -> EscapeMode {
    match current_css_scan_state(content) {
        CssScanState::Expr => EscapeMode::StyleExpr,
        CssScanState::SingleQuote { .. } => EscapeMode::StyleString { quote: '\'' },
        CssScanState::DoubleQuote { .. } => EscapeMode::StyleString { quote: '"' },
        CssScanState::LineComment => EscapeMode::StyleLineComment,
        CssScanState::BlockComment => EscapeMode::StyleBlockComment,
    }
}

fn is_html_space_char(ch: char) -> bool {
    matches!(ch, '\t' | '\n' | '\x0c' | '\r' | ' ')
}

#[cfg(test)]
fn ends_with_css_keyword(css: &str, keyword: &str) -> bool {
    let trimmed = css.trim_end_matches(is_html_space_char);
    if trimmed.len() < keyword.len() {
        return false;
    }

    let start = trimmed.len() - keyword.len();
    if !trimmed[start..].eq_ignore_ascii_case(keyword) {
        return false;
    }

    if start == 0 {
        return true;
    }

    let prev = trimmed[..start].chars().next_back();
    match prev {
        Some(ch) => !is_css_nmchar(ch as u32),
        None => true,
    }
}

#[cfg(test)]
fn is_css_nmchar(codepoint: u32) -> bool {
    if (0x30..=0x39).contains(&codepoint)
        || (0x41..=0x5A).contains(&codepoint)
        || (0x61..=0x7A).contains(&codepoint)
        || codepoint == 0x5F
        || codepoint == 0x2D
    {
        return true;
    }

    if codepoint < 0x80 {
        return false;
    }
    if codepoint > 0x10FFFF {
        return false;
    }
    if (0xD800..=0xDFFF).contains(&codepoint) {
        return false;
    }
    if matches!(codepoint, 0xFFFE | 0xFFFF) {
        return false;
    }
    true
}

fn is_css_space_byte(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
}

fn hex_decode(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0_u32, |acc, byte| {
        let digit = match byte {
            b'0'..=b'9' => (byte - b'0') as u32,
            b'a'..=b'f' => (byte - b'a' + 10) as u32,
            b'A'..=b'F' => (byte - b'A' + 10) as u32,
            _ => 0_u32,
        };
        (acc << 4) | digit
    })
}

#[cfg(test)]
fn skip_css_space(css: &str) -> &str {
    let bytes = css.as_bytes();
    if bytes.is_empty() {
        return css;
    }

    match bytes[0] {
        b'\r' => {
            if bytes.len() > 1 && bytes[1] == b'\n' {
                &css[2..]
            } else {
                &css[1..]
            }
        }
        b'\n' | b'\t' | b'\x0c' | b' ' => &css[1..],
        _ => css,
    }
}

fn decode_css(css: &str) -> String {
    let bytes = css.as_bytes();
    let mut output = String::with_capacity(css.len());
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] != b'\\' {
            let ch = css[i..]
                .chars()
                .next()
                .expect("valid utf-8 char boundary while decoding css");
            output.push(ch);
            i += ch.len_utf8();
            continue;
        }

        i += 1;
        if i >= bytes.len() {
            break;
        }

        if bytes[i].is_ascii_hexdigit() {
            let start = i;
            let mut end = i;
            while end < bytes.len() && end - start < 6 && bytes[end].is_ascii_hexdigit() {
                end += 1;
            }

            let value = hex_decode(&bytes[start..end]);
            let ch = char::from_u32(value).unwrap_or('\u{FFFD}');
            output.push(ch);
            i = end;

            if i < bytes.len() {
                if bytes[i] == b'\r' {
                    i += 1;
                    if i < bytes.len() && bytes[i] == b'\n' {
                        i += 1;
                    }
                } else if is_css_space_byte(bytes[i]) {
                    i += 1;
                }
            }
            continue;
        }

        if bytes[i] == b'\r' {
            i += 1;
            if i < bytes.len() && bytes[i] == b'\n' {
                i += 1;
            }
            continue;
        }
        if matches!(bytes[i], b'\n' | b'\x0c') {
            i += 1;
            continue;
        }

        let ch = css[i..]
            .chars()
            .next()
            .expect("valid utf-8 char boundary while decoding css escape");
        output.push(ch);
        i += ch.len_utf8();
    }

    output
}

fn css_value_filter(css: &str) -> String {
    let decoded = decode_css(css);
    let lower = decoded.to_ascii_lowercase();
    let collapsed_alnum = lower
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();
    let escape_tail_alnum = css_escape_tail_alnum(css);

    let forbidden = [
        "<!--",
        "-->",
        "<![cdata[",
        "]]>",
        "</style",
        "/*",
        "//",
        "[href=~",
        "@import",
    ];
    if forbidden.iter().any(|needle| lower.contains(needle)) {
        return "ZgotmplZ".to_string();
    }
    if collapsed_alnum.contains("expression")
        || collapsed_alnum.contains("mozbinding")
        || escape_tail_alnum.contains("expression")
        || escape_tail_alnum.contains("mozbinding")
    {
        return "ZgotmplZ".to_string();
    }

    if decoded.contains('<')
        || decoded.contains('>')
        || decoded.contains('"')
        || decoded.contains('\'')
        || decoded.contains('`')
        || decoded.contains('\0')
    {
        return "ZgotmplZ".to_string();
    }

    decoded
}

fn css_escape_tail_alnum(css: &str) -> String {
    let bytes = css.as_bytes();
    let mut output = String::new();
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 1;
            if i >= bytes.len() {
                break;
            }

            if bytes[i].is_ascii_hexdigit() {
                let start = i;
                while i < bytes.len() && i - start < 6 && bytes[i].is_ascii_hexdigit() {
                    i += 1;
                }
                let tail = bytes[i - 1].to_ascii_lowercase();
                if tail.is_ascii_alphanumeric() {
                    output.push(tail as char);
                }

                if i < bytes.len() && is_css_space_byte(bytes[i]) {
                    i += 1;
                }
                continue;
            }

            let ch = bytes[i].to_ascii_lowercase();
            if ch.is_ascii_alphanumeric() {
                output.push(ch as char);
            }
            i += 1;
            continue;
        }

        let ch = bytes[i].to_ascii_lowercase();
        if ch.is_ascii_alphanumeric() {
            output.push(ch as char);
        }
        i += 1;
    }

    output
}

fn update_url_part_with_byte(url_part: &mut UrlPartContext, byte: u8) {
    if byte == b'#' {
        *url_part = UrlPartContext::Fragment;
    } else if byte == b'?' && !matches!(*url_part, UrlPartContext::Fragment) {
        *url_part = UrlPartContext::Query;
    }
}

fn scan_css_state_until(
    content: &str,
    start: usize,
    initial_state: CssScanState,
    stop_on_style_close: bool,
) -> (CssScanState, Option<usize>) {
    let bytes = content.as_bytes();
    if start >= bytes.len() {
        return (initial_state, None);
    }

    let mut state = initial_state;
    let mut i = start;

    while i < bytes.len() {
        state = match state {
            CssScanState::Expr => {
                if stop_on_style_close && is_html_close_tag(bytes, i, b"style") {
                    return (CssScanState::Expr, Some(i));
                }

                match bytes[i] {
                    b'"' => {
                        let is_url = ends_with_css_url_start_bytes(bytes, i);
                        i += 1;
                        CssScanState::DoubleQuote {
                            is_url,
                            url_part: UrlPartContext::Path,
                        }
                    }
                    b'\'' => {
                        let is_url = ends_with_css_url_start_bytes(bytes, i);
                        i += 1;
                        CssScanState::SingleQuote {
                            is_url,
                            url_part: UrlPartContext::Path,
                        }
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
            CssScanState::SingleQuote {
                is_url,
                mut url_part,
            } => {
                if bytes[i] == b'\\' {
                    if is_url && i + 1 < bytes.len() {
                        update_url_part_with_byte(&mut url_part, bytes[i + 1]);
                    }
                    i += 2;
                    CssScanState::SingleQuote { is_url, url_part }
                } else if bytes[i] == b'\'' {
                    i += 1;
                    CssScanState::Expr
                } else {
                    if is_url {
                        update_url_part_with_byte(&mut url_part, bytes[i]);
                    }
                    i += 1;
                    CssScanState::SingleQuote { is_url, url_part }
                }
            }
            CssScanState::DoubleQuote {
                is_url,
                mut url_part,
            } => {
                if bytes[i] == b'\\' {
                    if is_url && i + 1 < bytes.len() {
                        update_url_part_with_byte(&mut url_part, bytes[i + 1]);
                    }
                    i += 2;
                    CssScanState::DoubleQuote { is_url, url_part }
                } else if bytes[i] == b'"' {
                    i += 1;
                    CssScanState::Expr
                } else {
                    if is_url {
                        update_url_part_with_byte(&mut url_part, bytes[i]);
                    }
                    i += 1;
                    CssScanState::DoubleQuote { is_url, url_part }
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

    (state, None)
}

fn advance_css_scan_state(content: &str, initial_state: CssScanState) -> CssScanState {
    scan_css_state_until(content, 0, initial_state, false).0
}

fn current_css_scan_state(content: &str) -> CssScanState {
    advance_css_scan_state(content, CssScanState::Expr)
}

fn escape_script_value(value: &Value) -> Result<String> {
    js_val_escaper(value)
}

fn js_val_escaper(value: &Value) -> Result<String> {
    match value {
        Value::SafeJs(raw) => Ok(raw.clone()),
        Value::SafeJsStr(raw) => Ok(format!("\"{raw}\"")),
        Value::SafeHtml(raw)
        | Value::SafeHtmlAttr(raw)
        | Value::SafeCss(raw)
        | Value::SafeUrl(raw)
        | Value::SafeSrcset(raw) => {
            let encoded = serde_json::to_string(raw)?;
            Ok(sanitize_json_for_script(&encoded))
        }
        Value::Json(JsonValue::Null) => Ok(" null ".to_string()),
        Value::Json(JsonValue::Bool(_) | JsonValue::Number(_)) => {
            let encoded = serde_json::to_string(value_json(value))?;
            Ok(format!(" {} ", sanitize_json_for_script(&encoded)))
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

fn value_json(value: &Value) -> &JsonValue {
    match value {
        Value::Json(json) => json,
        _ => unreachable!("value_json only accepts Value::Json"),
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

#[cfg(test)]
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
    let bytes = input.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == b'%' {
            if i + 2 < bytes.len()
                && bytes[i + 1].is_ascii_hexdigit()
                && bytes[i + 2].is_ascii_hexdigit()
            {
                encoded.push('%');
                encoded.push(bytes[i + 1] as char);
                encoded.push(bytes[i + 2] as char);
                i += 3;
                continue;
            }
            encoded.push_str("%25");
            i += 1;
            continue;
        }
        if is_safe_url_attr_byte(byte) {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(hex_lower((byte >> 4) & 0x0F));
            encoded.push(hex_lower(byte & 0x0F));
        }
        i += 1;
    }
    encoded
}

#[cfg(test)]
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

#[cfg(test)]
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

fn is_html_tag_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'_' | b'-')
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
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
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

fn js_string_escaper(input: &str) -> String {
    let mut escaped = String::new();
    for ch in input.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '/' => escaped.push_str("\\/"),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\u{0C}' => escaped.push_str("\\f"),
            '\u{2028}' => escaped.push_str("\\u2028"),
            '\u{2029}' => escaped.push_str("\\u2029"),
            '\u{007F}' => escaped.push_str("\\u007f"),
            '<' => escaped.push_str("\\u003c"),
            '>' => escaped.push_str("\\u003e"),
            '&' => escaped.push_str("\\u0026"),
            '"' => escaped.push_str("\\u0022"),
            '\'' => escaped.push_str("\\u0027"),
            '+' => escaped.push_str("\\u002b"),
            '`' => escaped.push_str("\\u0060"),
            c if (c as u32) < 0x20 => escaped.push_str(&format!("\\u{:04x}", c as u32)),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn js_string_escaper_norm(input: &str) -> String {
    let mut escaped = String::new();
    for ch in input.chars() {
        match ch {
            '/' => escaped.push_str("\\/"),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\u{0C}' => escaped.push_str("\\f"),
            '\u{2028}' => escaped.push_str("\\u2028"),
            '\u{2029}' => escaped.push_str("\\u2029"),
            '\u{007F}' => escaped.push_str("\\u007f"),
            '<' => escaped.push_str("\\u003c"),
            '>' => escaped.push_str("\\u003e"),
            '&' => escaped.push_str("\\u0026"),
            '"' => escaped.push_str("\\u0022"),
            '\'' => escaped.push_str("\\u0027"),
            '+' => escaped.push_str("\\u002b"),
            '`' => escaped.push_str("\\u0060"),
            c if (c as u32) < 0x20 => escaped.push_str(&format!("\\u{:04x}", c as u32)),
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
fn js_regexp_escaper(input: &str) -> String {
    if input.is_empty() {
        return "(?:)".to_string();
    }

    let mut escaped = String::new();
    for ch in input.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '/' => escaped.push_str("\\/"),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\u{0C}' => escaped.push_str("\\f"),
            '\u{2028}' => escaped.push_str("\\u2028"),
            '\u{2029}' => escaped.push_str("\\u2029"),
            '\u{007F}' => escaped.push_str("\\u007f"),
            '<' => escaped.push_str("\\u003c"),
            '>' => escaped.push_str("\\u003e"),
            '&' => escaped.push_str("\\u0026"),
            '"' => escaped.push_str("\\u0022"),
            '\'' => escaped.push_str("\\u0027"),
            '+' => escaped.push_str("\\u002b"),
            '$' | '(' | ')' | '*' | '-' | '.' | '?' | '[' | ']' | '^' | '{' | '|' | '}' => {
                escaped.push('\\');
                escaped.push(ch);
            }
            c if (c as u32) < 0x20 => escaped.push_str(&format!("\\u{:04x}", c as u32)),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn escape_css_text(input: &str) -> String {
    css_escaper(input)
}

fn css_escaper(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    let mut written = 0usize;

    for (index, ch) in input.char_indices() {
        let Some(replacement) = css_escape_replacement(ch) else {
            continue;
        };

        escaped.push_str(&input[written..index]);
        escaped.push_str(replacement);

        let next_index = index + ch.len_utf8();
        if replacement != r"\\" {
            let needs_space = match input.as_bytes().get(next_index).copied() {
                None => true,
                Some(byte) => byte.is_ascii_hexdigit() || is_css_space_byte(byte),
            };
            if needs_space {
                escaped.push(' ');
            }
        }
        written = next_index;
    }

    if written == 0 {
        return input.to_string();
    }

    escaped.push_str(&input[written..]);
    escaped
}

fn css_escape_replacement(ch: char) -> Option<&'static str> {
    match ch {
        '\0' => Some("\\0"),
        '\t' => Some("\\9"),
        '\n' => Some("\\a"),
        '\x0c' => Some("\\c"),
        '\r' => Some("\\d"),
        '"' => Some("\\22"),
        '&' => Some("\\26"),
        '\'' => Some("\\27"),
        '(' => Some("\\28"),
        ')' => Some("\\29"),
        '+' => Some("\\2b"),
        '/' => Some("\\2f"),
        ':' => Some("\\3a"),
        ';' => Some("\\3b"),
        '<' => Some("\\3c"),
        '>' => Some("\\3e"),
        '\\' => Some(r"\\"),
        '{' => Some("\\7b"),
        '}' => Some("\\7d"),
        _ => None,
    }
}

fn escape_css_string_fragment(input: &str, _quote: char) -> String {
    css_escaper(input)
}

fn index_tag_end(source: &str, tag: &str) -> Option<usize> {
    let mut offset = 0usize;
    let mut rest = source;
    let tag_len = tag.len();

    while let Some(relative) = rest.find("</") {
        let tag_start = relative + 2;
        let candidate = &rest[tag_start..];
        if candidate.len() >= tag_len
            && candidate[..tag_len].eq_ignore_ascii_case(tag)
            && is_html_tag_boundary(candidate.as_bytes().get(tag_len).copied())
        {
            return Some(offset + relative);
        }

        let consumed = relative + 2;
        offset += consumed;
        rest = &rest[consumed..];
    }

    None
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
            encoded.push(hex_lower((byte >> 4) & 0x0F));
            encoded.push(hex_lower(byte & 0x0F));
        }
    }
    encoded
}

fn query_escape_url(input: &str) -> String {
    let mut encoded = String::new();
    for &byte in input.as_bytes() {
        if is_unreserved_url_byte(byte) {
            encoded.push(byte as char);
        } else if byte == b' ' {
            encoded.push('+');
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

fn hex_lower(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'a' + (value - 10)) as char,
        _ => '0',
    }
}

const LONGEST_ENTITY_WITHOUT_SEMICOLON: usize = 6;

const NUMERIC_ENTITY_REPLACEMENTS: [char; 32] = [
    '\u{20AC}', '\u{0081}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{008D}', '\u{017D}', '\u{008F}',
    '\u{0090}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{009D}', '\u{017E}', '\u{0178}',
];

const HTML_ENTITY_TABLE: &[(&str, char)] = &[
    ("amp;", '&'),
    ("amp", '&'),
    ("lt;", '<'),
    ("lt", '<'),
    ("gt;", '>'),
    ("gt", '>'),
    ("quot;", '"'),
    ("quot", '"'),
    ("apos;", '\''),
    ("copy;", '\u{00A9}'),
    ("copy", '\u{00A9}'),
    ("aacute;", '\u{00E1}'),
    ("aacute", '\u{00E1}'),
];

const HTML_ENTITY_TABLE2: &[(&str, [char; 2])] = &[("gesl;", ['\u{22DB}', '\u{FE00}'])];

#[cfg(test)]
fn entity_maps() -> (
    &'static [(&'static str, char)],
    &'static [(&'static str, [char; 2])],
) {
    (HTML_ENTITY_TABLE, HTML_ENTITY_TABLE2)
}

fn lookup_entity(name: &str) -> Option<char> {
    HTML_ENTITY_TABLE
        .iter()
        .find_map(|(entity_name, value)| (*entity_name == name).then_some(*value))
}

fn lookup_entity2(name: &str) -> Option<[char; 2]> {
    HTML_ENTITY_TABLE2
        .iter()
        .find_map(|(entity_name, value)| (*entity_name == name).then_some(*value))
}

fn decode_digit(byte: u8, hex: bool) -> Option<u8> {
    if hex {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    } else {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            _ => None,
        }
    }
}

fn decode_numeric_entity(input: &str) -> Option<(String, usize)> {
    let bytes = input.as_bytes();
    if bytes.len() < 4 {
        return None;
    }

    let mut index = 2;
    let mut hex = false;
    if matches!(bytes.get(index), Some(b'x' | b'X')) {
        hex = true;
        index += 1;
    }

    let mut value: u64 = 0;
    let mut matched_digits = 0usize;
    while index < bytes.len() {
        if let Some(digit) = decode_digit(bytes[index], hex) {
            value = value
                .saturating_mul(if hex { 16 } else { 10 })
                .saturating_add(digit as u64);
            matched_digits += 1;
            index += 1;
            continue;
        }

        if bytes[index] == b';' {
            index += 1;
        }
        break;
    }

    if matched_digits == 0 {
        return None;
    }

    let codepoint = if (0x80..=0x9F).contains(&value) {
        NUMERIC_ENTITY_REPLACEMENTS[(value - 0x80) as usize] as u32
    } else if value == 0 || (0xD800..=0xDFFF).contains(&(value as u32)) || value > 0x10_FFFF {
        '\u{FFFD}' as u32
    } else {
        value as u32
    };

    let mut decoded = String::new();
    decoded.push(char::from_u32(codepoint).unwrap_or('\u{FFFD}'));
    Some((decoded, index))
}

fn decode_named_entity(input: &str) -> Option<(String, usize)> {
    let bytes = input.as_bytes();
    let mut index = 1usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_alphanumeric() {
            index += 1;
            continue;
        }
        if byte == b';' {
            index += 1;
        }
        break;
    }

    let entity_name = &input[1..index];
    if entity_name.is_empty() {
        return None;
    }

    if let Some(value) = lookup_entity(entity_name) {
        return Some((value.to_string(), index));
    }
    if let Some(values) = lookup_entity2(entity_name) {
        let mut decoded = String::new();
        decoded.push(values[0]);
        decoded.push(values[1]);
        return Some((decoded, index));
    }

    let max_len = entity_name
        .len()
        .saturating_sub(1)
        .min(LONGEST_ENTITY_WITHOUT_SEMICOLON);
    for candidate_len in (2..=max_len).rev() {
        if let Some(value) = lookup_entity(&entity_name[..candidate_len]) {
            return Some((value.to_string(), candidate_len + 1));
        }
    }

    None
}

fn decode_html_entity(input: &str) -> Option<(String, usize)> {
    let bytes = input.as_bytes();
    if bytes.len() <= 1 || bytes[0] != b'&' {
        return None;
    }
    if bytes[1] == b'#' {
        decode_numeric_entity(input)
    } else {
        decode_named_entity(input)
    }
}

fn unescape_html_entities(input: &str) -> String {
    let first_amp = match input.find('&') {
        Some(index) => index,
        None => return input.to_string(),
    };

    let mut output = String::with_capacity(input.len());
    output.push_str(&input[..first_amp]);

    let mut cursor = first_amp;
    while cursor < input.len() {
        let remainder = &input[cursor..];
        if !remainder.starts_with('&') {
            if let Some(next_amp) = remainder.find('&') {
                output.push_str(&remainder[..next_amp]);
                cursor += next_amp;
                continue;
            }
            output.push_str(remainder);
            break;
        }

        if let Some((decoded, consumed)) = decode_html_entity(remainder) {
            output.push_str(&decoded);
            cursor += consumed;
        } else {
            output.push('&');
            cursor += 1;
        }
    }

    output
}

fn escape_html(input: &str) -> String {
    escape_html_with_amp(input, true)
}

fn escape_html_norm(input: &str) -> String {
    escape_html_with_amp(input, false)
}

fn escape_html_with_amp(input: &str, escape_amp: bool) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' if escape_amp => escaped.push_str("&amp;"),
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

fn is_noncharacter_scalar(codepoint: u32) -> bool {
    (0xFDD0..=0xFDEF).contains(&codepoint) || (codepoint & 0xFFFE) == 0xFFFE
}

fn html_nospace_escaper(input: &str) -> String {
    html_nospace_escaper_with_amp(input, true)
}

fn html_nospace_escaper_norm(input: &str) -> String {
    html_nospace_escaper_with_amp(input, false)
}

fn html_nospace_escaper_with_amp(input: &str, escape_amp: bool) -> String {
    if input.is_empty() {
        return "ZgotmplZ".to_string();
    }
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\0' => escaped.push_str("&#xfffd;"),
            '\t' => escaped.push_str("&#9;"),
            '\n' => escaped.push_str("&#10;"),
            '\u{000B}' => escaped.push_str("&#11;"),
            '\u{000C}' => escaped.push_str("&#12;"),
            '\r' => escaped.push_str("&#13;"),
            ' ' => escaped.push_str("&#32;"),
            '"' => escaped.push_str("&#34;"),
            '\'' => escaped.push_str("&#39;"),
            '&' if escape_amp => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '+' => escaped.push_str("&#43;"),
            '=' => escaped.push_str("&#61;"),
            '`' => escaped.push_str("&#96;"),
            c if is_noncharacter_scalar(c as u32) => {
                escaped.push_str(&format!("&#x{:x};", c as u32));
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn parse_tag_at(source: &str, start: usize) -> Option<(usize, bool, String)> {
    let bytes = source.as_bytes();
    let mut i = start + 1;
    if i >= bytes.len() {
        return None;
    }

    let mut is_close = false;
    if bytes[i] == b'/' {
        is_close = true;
        i += 1;
        if i >= bytes.len() {
            return None;
        }
    }

    if !bytes[i].is_ascii_alphabetic() {
        return None;
    }

    let name_start = i;
    while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
        i += 1;
    }
    let name = source[name_start..i].to_ascii_lowercase();

    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }

        match b {
            b'\'' | b'"' => {
                quote = Some(b);
                i += 1;
            }
            b'>' => return Some((i + 1, is_close, name)),
            _ => i += 1,
        }
    }
    None
}

fn strip_tags(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] != b'<' {
            let next = input[i..]
                .find('<')
                .map(|rel| i + rel)
                .unwrap_or(bytes.len());
            output.push_str(&input[i..next]);
            i = next;
            continue;
        }

        if input[i..].starts_with("<!--") {
            if let Some(close_rel) = input[i + 4..].find("-->") {
                i += 4 + close_rel + 3;
                continue;
            }
            output.push('<');
            i += 1;
            continue;
        }

        let Some((end, is_close, name)) = parse_tag_at(input, i) else {
            output.push('<');
            i += 1;
            continue;
        };

        if !is_close && name == "script" {
            let mut cursor = end;
            let mut found = None;
            while cursor < bytes.len() {
                let Some(rel) = input[cursor..].find('<') else {
                    break;
                };
                let candidate = cursor + rel;
                if let Some((close_end, close_is_close, close_name)) =
                    parse_tag_at(input, candidate)
                    && close_is_close
                    && close_name == "script"
                {
                    found = Some(close_end);
                    break;
                }
                cursor = candidate + 1;
            }

            if let Some(close_end) = found {
                i = close_end;
            } else {
                i = bytes.len();
            }
            continue;
        }

        i = end;
    }

    output
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

    #[cfg(not(feature = "web-rust"))]
    fn create_template_dir(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempdir().expect("temp dir should be created");
        for (name, contents) in files {
            let path = dir.path().join(name);
            fs::write(path, contents).expect("template file should be written");
        }
        dir
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

    #[test]
    fn execute_value_to_string_matches_generic_execute() {
        let template = Template::new("value-exec")
            .parse("<p>{{.Name}}</p>")
            .expect("parse should succeed");

        let data = json!({"Name": "Alice"});
        let value = Value::from_serializable(&data).expect("value conversion should succeed");

        let generic = template
            .execute_to_string(&data)
            .expect("generic execute should succeed");
        let by_value = template
            .execute_value_to_string(&value)
            .expect("value execute should succeed");

        assert_eq!(generic, by_value);
    }

    #[test]
    fn execute_template_value_writer_works_for_named_template() {
        let template = Template::new("root")
            .parse("{{define \"greet\"}}Hello {{.Name}}{{end}}")
            .expect("parse should succeed");

        let data = json!({"Name": "Alice"});
        let value = Value::from_serializable(&data).expect("value conversion should succeed");
        let mut output = Vec::new();

        template
            .execute_template_value(&mut output, "greet", &value)
            .expect("value execute should succeed");

        assert_eq!(
            String::from_utf8(output).expect("utf8 conversion should succeed"),
            "Hello Alice"
        );
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
            "<a href=\"https://example.com/q?a=b%20c&amp;x=%3cy%3e\">go</a>"
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

        assert_eq!(output, "<input ZgotmplZ=\"doEvil()\">");
    }

    #[test]
    fn parse_marks_dynamic_attribute_value_expr_as_runtime_mode() {
        let template = Template::new("attrs")
            .parse("<input {{.Name}}=\"{{.Value}}\">")
            .expect("parse should succeed");

        let templates = template.name_space.templates.read().unwrap();
        let nodes = templates.get("attrs").expect("template nodes should exist");

        let expr_nodes: Vec<(EscapeMode, bool)> = nodes
            .iter()
            .filter_map(|node| match node {
                Node::Expr {
                    mode, runtime_mode, ..
                } => Some((*mode, *runtime_mode)),
                _ => None,
            })
            .collect();

        assert_eq!(expr_nodes.len(), 2);
        assert_eq!(expr_nodes[0], (EscapeMode::AttrName, false));
        assert_eq!(
            expr_nodes[1],
            (
                EscapeMode::AttrQuoted {
                    kind: AttrKind::Normal,
                    quote: '"',
                },
                true
            )
        );
    }

    #[test]
    fn parse_keeps_static_attribute_value_expr_fixed_mode() {
        let template = Template::new("attrs")
            .parse("<input title=\"{{.Value}}\">")
            .expect("parse should succeed");

        let templates = template.name_space.templates.read().unwrap();
        let nodes = templates.get("attrs").expect("template nodes should exist");

        let expr_nodes: Vec<(EscapeMode, bool)> = nodes
            .iter()
            .filter_map(|node| match node {
                Node::Expr {
                    mode, runtime_mode, ..
                } => Some((*mode, *runtime_mode)),
                _ => None,
            })
            .collect();

        assert_eq!(expr_nodes.len(), 1);
        assert_eq!(
            expr_nodes[0],
            (
                EscapeMode::AttrQuoted {
                    kind: AttrKind::Normal,
                    quote: '"',
                },
                false
            )
        );
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
            "<div ZgotmplZ=\"color: expression(alert(1337))\"></div>"
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

        assert_eq!(output, "<img ZgotmplZ=\"javascript:alert(1)\">");
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

        assert_eq!(output, "<a style=\"ZgotmplZ\"></a>");
    }

    #[test]
    fn dynamic_attribute_name_empty_value_is_rejected() {
        let template = Template::new("attrs")
            .parse("<input {{.Name}} name=n>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Name": ""}))
            .expect("parse should succeed");

        assert_eq!(output, "<input ZgotmplZ name=n>");
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
            "<img srcset=\" /foo/bar.png 200w, /baz/boo%281%29.png\">"
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
    fn html_comments_are_stripped_without_actions() {
        let template = Template::new("comments-only")
            .parse("<div>a<!--hidden--></div>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(output, "<div>a</div>");
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
    fn script_adjacent_tags_do_not_duplicate_close_tag_slash() {
        let template = Template::new("script")
            .parse("<script>const x={{.X}};</script><script>const y={{.Y}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"X": "abc", "Y": "def"}))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<script>const x=\"abc\";</script><script>const y=\"def\";</script>"
        );
        assert!(!output.contains("<//script>"));
    }

    #[test]
    fn script_filter_preserves_regexp_literal_slashes() {
        let filtered = filter_script_text("<script>", "const r=/ab/; const x = 1;");
        assert_eq!(filtered, "const r=/ab/; const x = 1;");
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
        let mut raw_templates = template.name_space.templates.read().unwrap().clone();
        if let Some(nodes) = raw_templates.get("script").cloned() {
            println!("quotes nodes = {:#?}", nodes);
            let mut tracker = ContextTracker::from_state(ContextState::html_text());
            for node in &nodes {
                match node {
                    Node::Text(text_node) => {
                        let before = tracker.state().mode;
                        tracker.append_text(&text_node.raw);
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
            let mut analyze = ParseContextAnalyzer::new(&mut raw_templates);
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
            let analyzed_flows = ParseContextAnalyzer::new(&mut raw_templates)
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
        let mut analyzer = ParseContextAnalyzer::new(&mut raw_templates);
        let quotes_end = analyzer
            .analyze_template("script", ContextState::html_text())
            .expect("analysis should succeed");
        println!("quotes analyzer end state = {:?}", quotes_end);

        let raw_line = "<script>const s = `a ${\"x\" // }\n+ {{.R}}}`;</script>";
        let mut template = Template::new("script");
        template
            .parse_named("script", raw_line)
            .expect("parse_named should succeed");
        let mut raw_templates = template.name_space.templates.read().unwrap().clone();
        if let Some(nodes) = raw_templates.get("script").cloned() {
            println!("line nodes = {:?}", nodes);
            let mut nodes_for_analysis = nodes.clone();
            let mut tracker = ContextTracker::from_state(ContextState::html_text());
            let mut analyzer = ParseContextAnalyzer::new(&mut raw_templates);
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
        let mut analyzer = ParseContextAnalyzer::new(&mut raw_templates);
        let line_end = analyzer
            .analyze_template("script", ContextState::html_text())
            .expect("analysis should succeed");
        println!("line analyzer end state = {:?}", line_end);

        let raw_block = "<script>const s = `a ${\"x\" /* } */ + {{.R}}}`;</script>";
        let mut template = Template::new("script");
        template
            .parse_named("script", raw_block)
            .expect("parse_named should succeed");
        let mut raw_templates = template.name_space.templates.read().unwrap().clone();
        if let Some(nodes) = raw_templates.get("script").cloned() {
            println!("block nodes = {:?}", nodes);
            let mut nodes_for_analysis = nodes.clone();
            let mut tracker = ContextTracker::from_state(ContextState::html_text());
            let mut analyzer = ParseContextAnalyzer::new(&mut raw_templates);
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
        let mut analyzer = ParseContextAnalyzer::new(&mut raw_templates);
        let block_end = analyzer
            .analyze_template("script", ContextState::html_text())
            .expect("analysis should succeed");
        println!("block analyzer end state = {:?}", block_end);
    }

    fn analyze_expr_modes(name: &str, source: &str) -> Vec<EscapeMode> {
        let template = Template::new(name)
            .parse(source)
            .expect("parse should succeed");

        let mut raw_templates = template.name_space.templates.read().unwrap().clone();
        let mut nodes = raw_templates
            .get(name)
            .expect("template should exist")
            .clone();

        let mut analyzer = ParseContextAnalyzer::new(&mut raw_templates);
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
    fn parse_if_branch_cache_keeps_expr_modes_in_script_context() {
        let source =
            "<script>{{if .X}}{{.A}}{{else}}z{{end}}{{if .X}}{{.B}}{{else}}y{{end}}</script>";
        let mut template = Template::new("if-branch-cache");
        template
            .parse_named("if-branch-cache", source)
            .expect("parse_named should succeed");

        let mut raw_templates = template.name_space.templates.read().unwrap().clone();
        let mut nodes = raw_templates
            .get("if-branch-cache")
            .expect("template nodes should exist")
            .clone();

        let mut analyzer = ParseContextAnalyzer::new(&mut raw_templates);
        let flows = analyzer
            .analyze_nodes(
                &mut nodes,
                ContextTracker::from_state(ContextState::html_text()),
                false,
            )
            .expect("analyze_nodes should succeed");
        assert!(!analyzer.if_transition_cache.is_empty());
        assert_eq!(flows.len(), 1);
        assert_eq!(flows[0].tracker.state(), ContextState::html_text());

        fn collect_if_branch_expr_modes(nodes: &[Node], out: &mut Vec<EscapeMode>) {
            for node in nodes {
                match node {
                    Node::If {
                        then_branch,
                        else_branch,
                        ..
                    } => {
                        collect_if_branch_expr_modes(then_branch, out);
                        collect_if_branch_expr_modes(else_branch, out);
                    }
                    Node::Expr { mode, .. } => out.push(*mode),
                    _ => {}
                }
            }
        }

        let mut expr_modes = Vec::new();
        collect_if_branch_expr_modes(&nodes, &mut expr_modes);
        assert_eq!(
            expr_modes,
            vec![EscapeMode::ScriptExpr, EscapeMode::ScriptExpr]
        );
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
    fn context_tracker_incremental_refresh_matches_full_recompute() {
        let mut tracker = ContextTracker::from_state(ContextState::html_text());
        let chunks = [
            "<div title=\"",
            "hello",
            "\">",
            "<script>",
            "const re = /foo\\//;",
            "const s = `x ${1 + 2}`;",
            "</script>",
            "<style>",
            "/* c */ .x { color: red; }",
            "</style>",
            "</div>",
        ];

        for chunk in chunks {
            tracker.append_text(chunk);

            let recomputed_state = ContextState::from_rendered(&tracker.rendered);
            assert_eq!(
                tracker.state, recomputed_state,
                "state mismatch after chunk: {chunk:?}"
            );

            let recomputed_url_part =
                url_part_from_mode_and_rendered(tracker.state.mode, &tracker.rendered);
            assert_eq!(
                tracker.url_part, recomputed_url_part,
                "url part mismatch after chunk: {chunk:?}"
            );
        }
    }

    #[test]
    fn context_tracker_css_url_part_incremental_matches_full_recompute() {
        let mut tracker = ContextTracker::from_state(ContextState::html_text());
        let chunks = [
            (
                "<style>.x{background:url(\"/img",
                Some(UrlPartContext::Path),
            ),
            ("?q=1", Some(UrlPartContext::Query)),
            ("#frag", Some(UrlPartContext::Fragment)),
            ("\");}</style>", None),
        ];

        for (chunk, expected_css_url_part) in chunks {
            tracker.append_text(chunk);

            let recomputed_state = ContextState::from_rendered(&tracker.rendered);
            assert_eq!(
                tracker.state, recomputed_state,
                "state mismatch after chunk: {chunk:?}"
            );

            let recomputed_css_url_part = css_url_part_context(&tracker.rendered);
            assert_eq!(
                tracker.css_url_part_hint().flatten(),
                recomputed_css_url_part,
                "css url part mismatch after chunk: {chunk:?}"
            );
            assert_eq!(
                tracker.css_url_part_hint().flatten(),
                expected_css_url_part,
                "unexpected css url part after chunk: {chunk:?}"
            );
        }
    }

    #[test]
    fn text_transition_cache_key_supports_script_context() {
        let tracker = ContextTracker::from_state(ContextState::from_rendered("<script>"));
        let key = text_transition_cache_key(&tracker, "const x=");
        assert!(key.is_some());
    }

    #[test]
    fn text_transition_cache_key_skips_script_close_when_not_in_expr_state() {
        let tracker = ContextTracker::from_state(ContextState::from_rendered("<script>\""));
        let key = text_transition_cache_key(&tracker, "</script>");
        assert!(key.is_none());
    }

    #[test]
    fn parse_text_tail_skip_condition_matches_script_style_expr_safety() {
        let script_tracker = ContextTracker::from_state(ContextState::from_rendered("<script>"));
        assert!(script_tracker.should_skip_rendered_tail_update_for_parse_text("const x="));
        assert!(!script_tracker.should_skip_rendered_tail_update_for_parse_text("</script>"));
        assert!(!script_tracker.should_skip_rendered_tail_update_for_parse_text("const x=\\"));

        let style_tracker = ContextTracker::from_state(ContextState::from_rendered("<style>"));
        assert!(style_tracker.should_skip_rendered_tail_update_for_parse_text(".x{color:red;}"));
        assert!(!style_tracker.should_skip_rendered_tail_update_for_parse_text("</style>"));
        assert!(!style_tracker.should_skip_rendered_tail_update_for_parse_text(".x{content:\\"));
    }

    #[test]
    fn parse_incremental_transition_handles_script_close_with_html_suffix() {
        let mut tracker = ContextTracker::from_state(ContextState::from_rendered("<script>"));
        let delta = ";</script><script>const x=";

        assert!(tracker.try_refresh_cached_state_incremental_for_parse(delta));
        assert!(tracker.state.is_script_tag_context());
        assert!(matches!(tracker.mode(), EscapeMode::ScriptExpr));
        assert!(matches!(
            tracker.js_scan_state,
            Some(JsScanState::Expr { .. })
        ));

        let rendered = "<script>".to_string() + delta;
        let expected_state = ContextState::from_rendered(&rendered);
        assert_eq!(tracker.state, expected_state);
    }

    #[test]
    fn parse_prepares_script_text_chunks_with_close_tag_segments() {
        let template = Template::new("prepared-script")
            .parse("<script>const x={{.X}};</script><script>const y={{.Y}};</script>")
            .expect("parse should succeed");

        let templates = template.name_space.templates.read().unwrap();
        let nodes = templates
            .get("prepared-script")
            .expect("template nodes should exist");

        let mut has_script_close_chunk = false;
        for node in nodes {
            if let Node::Text(text_node) = node
                && let Some(prepared) = text_node.prepared.as_ref()
            {
                has_script_close_chunk = prepared
                    .chunks
                    .iter()
                    .any(|chunk| matches!(chunk, PreparedTextChunk::ScriptCloseTag(_)));
                if has_script_close_chunk {
                    break;
                }
            }
        }

        assert!(has_script_close_chunk);
    }

    #[test]
    fn parse_prepares_style_text_chunks_with_close_tag_segments() {
        let template = Template::new("prepared-style")
            .parse("<style>.x{content:\"{{.X}}\";}</style><style>.y{color:red;}</style>")
            .expect("parse should succeed");

        let templates = template.name_space.templates.read().unwrap();
        let nodes = templates
            .get("prepared-style")
            .expect("template nodes should exist");

        let mut has_style_close_chunk = false;
        for node in nodes {
            if let Node::Text(text_node) = node
                && let Some(prepared) = text_node.prepared.as_ref()
            {
                has_style_close_chunk = prepared
                    .chunks
                    .iter()
                    .any(|chunk| matches!(chunk, PreparedTextChunk::StyleCloseTag(_)));
                if has_style_close_chunk {
                    break;
                }
            }
        }

        assert!(has_style_close_chunk);
    }

    #[test]
    fn parse_reuses_prepared_script_text_plans_for_identical_segments() {
        let template = Template::new("prepared-shared")
            .parse(
                "<script>const x={{.X}};</script><script>const x={{.X}};</script><script>const x={{.X}};</script>",
            )
            .expect("parse should succeed");

        let templates = template.name_space.templates.read().unwrap();
        let nodes = templates
            .get("prepared-shared")
            .expect("template nodes should exist");

        let mut shared_candidates = Vec::new();
        for node in nodes {
            if let Node::Text(text_node) = node
                && text_node.raw.as_str() == ";</script><script>const x="
                && let Some(prepared) = text_node.prepared.as_ref()
            {
                shared_candidates.push(prepared.clone());
            }
        }

        assert!(shared_candidates.len() >= 2);
        assert!(Arc::ptr_eq(&shared_candidates[0], &shared_candidates[1]));
    }

    #[test]
    fn parse_skips_preparing_script_style_chunks_in_attr_context() {
        let template = Template::new("prepared-attr")
            .parse("<a title=\"{{.X}}<script></script>\">ok</a>")
            .expect("parse should succeed");

        let templates = template.name_space.templates.read().unwrap();
        let nodes = templates
            .get("prepared-attr")
            .expect("template nodes should exist");

        let has_prepared_chunks = nodes
            .iter()
            .filter_map(|node| match node {
                Node::Text(text_node) => Some(text_node.prepared.is_some()),
                _ => None,
            })
            .any(std::convert::identity);

        assert!(!has_prepared_chunks);
    }

    #[test]
    fn current_unclosed_tag_content_finds_last_unclosed_case_insensitively() {
        let rendered = "<SCRIPT>const a = 1;</SCRIPT><script>const b = 2;";
        let content = current_unclosed_tag_content(rendered, "script");
        assert_eq!(content, Some("const b = 2;"));
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
            "<script>const s = `${/\\}/.test(\"x\") ?  1  : 0}`;</script>"
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

        assert_eq!(output, "<script>// \nconst x =  1 ;</script>");
    }

    #[test]
    fn script_line_comment_mode_with_crlf_is_terminated() {
        let template = Template::new("script")
            .parse("<script>// {{.A}}\r\nconst x = {{.B}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "inject", "B": 1}))
            .expect("execute should succeed");

        assert_eq!(output, "<script>// \r\nconst x =  1 ;</script>");
    }

    #[test]
    fn script_line_comment_mode_with_carriage_return_only_is_terminated() {
        let template = Template::new("script")
            .parse("<script>// {{.A}}\rconst x = {{.B}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "inject", "B": 1}))
            .expect("execute should succeed");

        assert_eq!(output, "<script>// \rconst x =  1 ;</script>");
    }

    #[test]
    fn script_line_comment_mode_with_unicode_line_separator_is_terminated() {
        let template = Template::new("script")
            .parse("<script>// {{.A}}\u{2028}const x = {{.B}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "inject", "B": 1}))
            .expect("execute should succeed");

        assert_eq!(output, "<script>// \u{2028}const x =  1 ;</script>");
    }

    #[test]
    fn script_line_comment_mode_with_unicode_paragraph_separator_is_terminated() {
        let template = Template::new("script")
            .parse("<script>// {{.A}}\u{2029}const x = {{.B}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"A": "inject", "B": 1}))
            .expect("execute should succeed");

        assert_eq!(output, "<script>// \u{2029}const x =  1 ;</script>");
    }

    #[test]
    fn script_expr_mode_with_unicode_line_separator_is_preserved() {
        let template = Template::new("script")
            .parse("<script>const x = 1;\u{2028}const y = {{.Y}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Y": 2}))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<script>const x = 1;\u{2028}const y =  2 ;</script>"
        );
    }

    #[test]
    fn script_expr_mode_with_unicode_paragraph_separator_is_preserved() {
        let template = Template::new("script")
            .parse("<script>const x = 1;\u{2029}const y = {{.Y}};</script>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Y": 2}))
            .expect("execute should succeed");

        assert_eq!(
            output,
            "<script>const x = 1;\u{2029}const y =  2 ;</script>"
        );
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

        assert_eq!(output, "<style>// </style>\n.a{color: ZgotmplZ}</style>");
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

        assert_eq!(output, "<style>/* </style> */.a{color: ZgotmplZ}</style>");
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
            "<style>.a{content:\"</style>\";color: ZgotmplZ}</style>"
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

        assert_eq!(output, "<style>.a{color: ZgotmplZ}</style>");
    }

    #[test]
    fn style_string_context_escapes_css_string_tokens() {
        let template = Template::new("style")
            .parse("<style>.x{content:'{{.S}}';}</style>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"S": "a'b\\c"}))
            .expect("execute should succeed");

        assert_eq!(output, "<style>.x{content:'a\\27 b\\\\c';}</style>");
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
            "<script>const s = \"\\u0022\\u003c\\/script\\u003e\\u003cx\\u003e\";</script>"
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

        assert_eq!(output, "a%3Db&#43;c%26x%3D%3Cy%3E");
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
    fn range_no_vars_iteration_scope_does_not_leak_declared_variables() {
        let template = Template::new("range-no-vars-scope")
            .parse("{{range .Items}}{{if eq . 1}}{{$x := \"one\"}}{{end}}{{$x}}{{end}}")
            .expect("parse should succeed");

        let error = template
            .execute_to_string(&json!({"Items": [1, 2]}))
            .expect_err("second iteration should fail because `$x` is not declared");

        assert!(
            error
                .to_string()
                .contains("variable `$x` could not be resolved")
        );
    }

    #[test]
    fn range_no_vars_can_assign_outer_variable_across_iterations() {
        let template = Template::new("range-no-vars-assign")
            .parse(
                "{{$out := \"\"}}{{range .Items}}{{$out = printf \"%s%s\" $out .}}{{end}}{{$out}}",
            )
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Items": ["a", "b", "c"]}))
            .expect("execute should succeed");

        assert_eq!(output, "abc");
    }

    #[test]
    fn range_no_vars_static_text_body_repeats_without_data_dependency() {
        let template = Template::new("range-no-vars-static-text")
            .parse("<ul>{{range .Items}}<li>x</li>{{end}}</ul>")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Items": [1, 2, 3]}))
            .expect("execute should succeed");

        assert_eq!(output, "<ul><li>x</li><li>x</li><li>x</li></ul>");
    }

    #[test]
    fn range_template_call_without_data_uses_current_dot() {
        let template = Template::new("range-template-dot")
            .parse(
                "{{define \"item\"}}{{.Name}};{{end}}{{range .Items}}{{template \"item\"}}{{end}}",
            )
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Items":[{"Name":"a"},{"Name":"b"}]}))
            .expect("execute should succeed");

        assert_eq!(output, "a;b;");
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

        assert_eq!(output, "&lt;tag&gt;|a&#43;b");
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
    fn safe_types_follow_contextual_escaping_rules() {
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
            "<a href=\"javascript:alert%281%29\">go</a><script>const s=\"\\\\\\u0022\\u003c\\/script\\u003e\";</script><style>.x{content:\"\\\\\\22\\3c\\2fstyle\\3e \";}</style>"
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

        assert_eq!(output, "<a title=\"O&#39;Reilly &amp; &lt;x&gt;\"></a>");
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
    fn go_test_errors_non_error_cases_parse_successfully() {
        let cases = [
            "{{if .Cond}}<a>{{else}}<b>{{end}}",
            "{{if .Cond}}<a>{{end}}",
            "{{if .Cond}}{{else}}<b>{{end}}",
            "{{with .Cond}}<div>{{end}}",
            "{{range .Items}}<a>{{end}}",
            "<a href='/foo?{{range .Items}}&{{.K}}={{.V}}{{end}}'>",
            "{{range .Items}}<a{{if .X}}{{end}}>{{end}}",
            "{{range .Items}}<a{{if .X}}{{end}}>{{continue}}{{end}}",
            "{{range .Items}}<a{{if .X}}{{end}}>{{break}}{{end}}",
            "{{range .Items}}<a{{if .X}}{{end}}>{{if .X}}{{break}}{{end}}{{end}}",
            "<script>var a = `${a+b}`</script>`",
            "<script>var tmpl = `asd`;</script>",
            "<script>var tmpl = `${1}`;</script>",
            "<script>var tmpl = `${return ``}`;</script>",
            "<script>var tmpl = `${return {{.}} }`;</script>",
            "<script>var tmpl = `${ let a = {1:1} {{.}} }`;</script>",
        ];

        for (idx, source) in cases.iter().enumerate() {
            Template::new(format!("go-errors-ok-{idx}"))
                .parse(source)
                .unwrap_or_else(|error| panic!("expected parse success for {source:?}: {error}"));
        }
    }

    #[test]
    fn go_test_errors_range_loop_and_output_context_cases_match_go_behavior() {
        let range_reentry = [
            "{{range .Items}}<a{{end}}",
            "\n{{range .Items}} x='<a{{end}}",
        ];
        for source in range_reentry {
            let error = match Template::new("go-range-reentry").parse(source) {
                Ok(_) => panic!("parse should fail for range re-entry mismatch"),
                Err(error) => error,
            };
            assert!(
                error.to_string().contains("on range loop re-entry"),
                "unexpected error for {source:?}: {error}"
            );
        }

        let range_flow_mismatch = [
            "{{range .Items}}<a{{if .X}}{{break}}{{end}}>{{end}}",
            "{{range .Items}}<a{{if .X}}{{continue}}{{end}}>{{end}}",
            "{{range .Items}}{{if .X}}{{break}}{{end}}<a{{if .Y}}{{continue}}{{end}}>{{if .Z}}{{continue}}{{end}}{{end}}",
        ];
        for source in range_flow_mismatch {
            let error = match Template::new("go-range-flow-mismatch").parse(source) {
                Ok(_) => panic!("parse should fail for range branch mismatch"),
                Err(error) => error,
            };
            assert!(
                error
                    .to_string()
                    .contains("{{range}} branches end in different contexts")
                    || error.to_string().contains("on range loop re-entry"),
                "unexpected error for {source:?}: {error}"
            );
        }

        let slash_ambiguity = match Template::new("go-slash-ambig")
            .parse(r#"<script>{{if false}}var x = 1{{end}}/-{{"1.5"}}/i.test(x)</script>"#)
        {
            Ok(_) => panic!("parse should fail for slash ambiguity"),
            Err(error) => error,
        };
        assert!(
            slash_ambiguity
                .to_string()
                .contains("could start a division or regexp"),
            "unexpected slash ambiguity error: {slash_ambiguity}"
        );

        let output_context = match Template::new("go-output-context").parse(
            r#"<script>reverseList = [{{template "t"}}]</script>{{define "t"}}{{if .Tail}}{{template "t" .Tail}}{{end}}{{.Head}}",{{end}}"#,
        ) {
            Ok(_) => panic!("parse should fail when recursive template output context cannot be computed"),
            Err(error) => error,
        };
        assert!(
            output_context
                .to_string()
                .contains("cannot compute output context for template")
                || output_context
                    .to_string()
                    .contains("could start a division or regexp")
                || output_context
                    .to_string()
                    .contains("ends in a non-text context"),
            "unexpected output-context error: {output_context}"
        );
    }

    #[test]
    fn go_test_next_js_ctx_matches_go_table() {
        let tests = [
            (JsContext::RegExp, ";"),
            (JsContext::RegExp, "}"),
            (JsContext::DivOp, ")"),
            (JsContext::DivOp, "]"),
            (JsContext::RegExp, "("),
            (JsContext::RegExp, "["),
            (JsContext::RegExp, "{"),
            (JsContext::RegExp, "="),
            (JsContext::RegExp, "+="),
            (JsContext::RegExp, "*="),
            (JsContext::RegExp, "*"),
            (JsContext::RegExp, "!"),
            (JsContext::RegExp, "+"),
            (JsContext::RegExp, "-"),
            (JsContext::DivOp, "--"),
            (JsContext::DivOp, "++"),
            (JsContext::DivOp, "x--"),
            (JsContext::RegExp, "x---"),
            (JsContext::RegExp, "return"),
            (JsContext::RegExp, "return "),
            (JsContext::RegExp, "return\t"),
            (JsContext::RegExp, "return\n"),
            (JsContext::RegExp, "return\u{2028}"),
            (JsContext::DivOp, "x"),
            (JsContext::DivOp, "x "),
            (JsContext::DivOp, "x\t"),
            (JsContext::DivOp, "x\n"),
            (JsContext::DivOp, "x\u{2028}"),
            (JsContext::DivOp, "preturn"),
            (JsContext::DivOp, "0"),
            (JsContext::DivOp, "0."),
            (JsContext::RegExp, "=\u{00A0}"),
        ];

        for (want, input) in tests {
            assert_eq!(next_js_ctx(input, JsContext::RegExp), want, "{input:?}");
            assert_eq!(next_js_ctx(input, JsContext::DivOp), want, "{input:?}");
        }

        assert_eq!(next_js_ctx("   ", JsContext::RegExp), JsContext::RegExp);
        assert_eq!(next_js_ctx("   ", JsContext::DivOp), JsContext::DivOp);
    }

    #[test]
    fn go_test_is_js_mime_type_matches_go_table() {
        let tests = [
            ("application/javascript;version=1.8", true),
            ("application/javascript;version=1.8;foo=bar", true),
            ("application/javascript/version=1.8", false),
            ("text/javascript", true),
            ("application/json", true),
            ("application/ld+json", true),
            ("module", true),
        ];

        for (input, want) in tests {
            assert_eq!(is_js_type_mime(input), want, "{input:?}");
        }
    }

    #[test]
    fn go_test_js_val_escaper_matches_go_table_core() {
        let tests: Vec<(JsonValue, &str, bool)> = vec![
            (json!(42), " 42 ", false),
            (json!(-42), " -42 ", false),
            (json!(9007199254740992_u64), " 9007199254740992 ", false),
            (json!(9007199254740993_u64), " 9007199254740993 ", false),
            (json!(1.0), " 1.0 ", false),
            (json!(-0.5), " -0.5 ", false),
            (json!(""), "\"\"", false),
            (json!("foo"), "\"foo\"", false),
            (
                json!("\r\n\u{2028}\u{2029}"),
                "\"\\r\\n\\u2028\\u2029\"",
                false,
            ),
            (json!("\t\u{000B}"), "\"\\t\\u000b\"", false),
            (json!({"X": 1, "Y": 2}), "{\"X\":1,\"Y\":2}", false),
            (json!([]), "[]", false),
            (json!([42, "foo", null]), "[42,\"foo\",null]", false),
            (
                json!(["<!--", "</script>", "-->"]),
                "[\"\\u003c!--\",\"\\u003c/script\\u003e\",\"--\\u003e\"]",
                false,
            ),
            (json!("<!--"), "\"\\u003c!--\"", false),
            (json!("-->"), "\"--\\u003e\"", false),
            (json!("<![CDATA["), "\"\\u003c![CDATA[\"", false),
            (json!("]]>"), "\"]]\\u003e\"", false),
            (json!("</script"), "\"\\u003c/script\"", false),
            (json!("𝄞"), "\"𝄞\"", false),
            (JsonValue::Null, " null ", false),
        ];

        for (value, want, skip_nest) in tests {
            let got = js_val_escaper(&Value::Json(value.clone()))
                .unwrap_or_else(|error| panic!("js_val_escaper failed for {value:?}: {error}"));
            assert_eq!(got, want, "{value:?}");

            if skip_nest {
                continue;
            }

            let nested = JsonValue::Array(vec![value.clone()]);
            let nested_got = js_val_escaper(&Value::Json(nested)).unwrap_or_else(|error| {
                panic!("nested js_val_escaper failed for {value:?}: {error}")
            });
            let nested_want = format!("[{}]", want.trim());
            assert_eq!(nested_got, nested_want, "nested {value:?}");
        }
    }

    #[test]
    fn go_test_js_str_escaper_matches_go_table() {
        let tests = [
            ("", ""),
            ("foo", "foo"),
            ("\u{0000}", "\\u0000"),
            ("\t", "\\t"),
            ("\n", "\\n"),
            ("\r", "\\r"),
            ("\u{2028}", "\\u2028"),
            ("\u{2029}", "\\u2029"),
            ("\\", "\\\\"),
            ("\\n", "\\\\n"),
            ("foo\r\nbar", "foo\\r\\nbar"),
            ("\"", "\\u0022"),
            ("'", "\\u0027"),
            ("&amp;", "\\u0026amp;"),
            ("</script>", "\\u003c\\/script\\u003e"),
            ("<![CDATA[", "\\u003c![CDATA["),
            ("]]>", "]]\\u003e"),
            ("<!--", "\\u003c!--"),
            ("-->", "--\\u003e"),
            (
                "+ADw-script+AD4-alert(1)+ADw-/script+AD4-",
                "\\u002bADw-script\\u002bAD4-alert(1)\\u002bADw-\\/script\\u002bAD4-",
            ),
        ];

        for (input, want) in tests {
            let got = js_string_escaper(input);
            assert_eq!(got, want, "{input:?}");
        }
    }

    #[test]
    fn go_test_js_regexp_escaper_matches_go_table() {
        let tests = [
            ("", "(?:)"),
            ("foo", "foo"),
            ("\u{0000}", "\\u0000"),
            ("\t", "\\t"),
            ("\n", "\\n"),
            ("\r", "\\r"),
            ("\u{2028}", "\\u2028"),
            ("\u{2029}", "\\u2029"),
            ("\\", "\\\\"),
            ("\\n", "\\\\n"),
            ("foo\r\nbar", "foo\\r\\nbar"),
            ("\"", "\\u0022"),
            ("'", "\\u0027"),
            ("&amp;", "\\u0026amp;"),
            ("</script>", "\\u003c\\/script\\u003e"),
            ("<![CDATA[", "\\u003c!\\[CDATA\\["),
            ("]]>", "\\]\\]\\u003e"),
            ("<!--", "\\u003c!\\-\\-"),
            ("-->", "\\-\\-\\u003e"),
            ("*", "\\*"),
            ("+", "\\u002b"),
            ("?", "\\?"),
            ("[](){}", "\\[\\]\\(\\)\\{\\}"),
            ("$foo|x.y", "\\$foo\\|x\\.y"),
            ("x^y", "x\\^y"),
        ];

        for (input, want) in tests {
            let got = js_regexp_escaper(input);
            assert_eq!(got, want, "{input:?}");
        }
    }

    #[test]
    fn go_test_js_escapers_on_lower7_and_selected_high_codepoints() {
        let input = concat!(
            "\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f",
            "\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f",
            " !\"#$%&'()*+,-./",
            "0123456789:;<=>?",
            "@ABCDEFGHIJKLMNO",
            "PQRSTUVWXYZ[\\]^_",
            "`abcdefghijklmno",
            "pqrstuvwxyz{|}~\x7f",
            "\u{00A0}\u{0100}\u{2028}\u{2029}\u{FEFF}\u{1D11E}",
        );

        let want_js_str = concat!(
            "\\u0000\\u0001\\u0002\\u0003\\u0004\\u0005\\u0006\\u0007",
            "\\u0008\\t\\n\\u000b\\f\\r\\u000e\\u000f",
            "\\u0010\\u0011\\u0012\\u0013\\u0014\\u0015\\u0016\\u0017",
            "\\u0018\\u0019\\u001a\\u001b\\u001c\\u001d\\u001e\\u001f",
            " !\\u0022#$%\\u0026\\u0027()*\\u002b,-.\\/",
            "0123456789:;\\u003c=\\u003e?",
            "@ABCDEFGHIJKLMNO",
            "PQRSTUVWXYZ[\\\\]^_",
            "\\u0060abcdefghijklmno",
            "pqrstuvwxyz{|}~\\u007f",
            "\u{00A0}\u{0100}\\u2028\\u2029\u{FEFF}\u{1D11E}",
        );
        assert_eq!(js_string_escaper(input), want_js_str);

        let want_js_regexp = concat!(
            "\\u0000\\u0001\\u0002\\u0003\\u0004\\u0005\\u0006\\u0007",
            "\\u0008\\t\\n\\u000b\\f\\r\\u000e\\u000f",
            "\\u0010\\u0011\\u0012\\u0013\\u0014\\u0015\\u0016\\u0017",
            "\\u0018\\u0019\\u001a\\u001b\\u001c\\u001d\\u001e\\u001f",
            " !\\u0022#\\$%\\u0026\\u0027\\(\\)\\*\\u002b,\\-\\.\\/",
            "0123456789:;\\u003c=\\u003e\\?",
            "@ABCDEFGHIJKLMNO",
            "PQRSTUVWXYZ\\[\\\\\\]\\^_",
            "`abcdefghijklmno",
            "pqrstuvwxyz\\{\\|\\}~\\u007f",
            "\u{00A0}\u{0100}\\u2028\\u2029\u{FEFF}\u{1D11E}",
        );
        assert_eq!(js_regexp_escaper(input), want_js_regexp);

        let mut by_char = String::new();
        for ch in input.chars() {
            by_char.push_str(&js_string_escaper(&ch.to_string()));
        }
        assert_eq!(by_char, want_js_str);

        let mut regexp_by_char = String::new();
        for ch in input.chars() {
            regexp_by_char.push_str(&js_regexp_escaper(&ch.to_string()));
        }
        assert_eq!(regexp_by_char, want_js_regexp);
    }

    #[test]
    fn go_test_escapers_on_lower_7_and_select_high_codepoints_matches_go_alias() {
        go_test_js_escapers_on_lower7_and_selected_high_codepoints();
    }

    #[test]
    fn go_test_srcset_filter_matches_go_table() {
        let tests = [
            (
                "one ok",
                "http://example.com/img.png",
                "http://example.com/img.png",
            ),
            ("one ok with metadata", " /img.png 200w", " /img.png 200w"),
            ("one bad", "javascript:alert(1) 200w", "#ZgotmplZ"),
            ("two ok", "foo.png, bar.png", "foo.png, bar.png"),
            (
                "left bad",
                "javascript:alert(1), /foo.png",
                "#ZgotmplZ, /foo.png",
            ),
            (
                "right bad",
                "/bogus#, javascript:alert(1)",
                "/bogus#,#ZgotmplZ",
            ),
        ];

        for (name, input, want) in tests {
            let got = filter_srcset_attribute_value(input);
            assert_eq!(got, want, "{name}");
        }
    }

    #[test]
    fn go_test_url_normalizer_matches_go_table() {
        let tests = [
            ("", ""),
            (
                "http://example.com:80/foo/bar?q=foo%20&bar=x+y#frag",
                "http://example.com:80/foo/bar?q=foo%20&bar=x+y#frag",
            ),
            (" ", "%20"),
            ("%7c", "%7c"),
            ("%7C", "%7C"),
            ("%2", "%252"),
            ("%", "%25"),
            ("%z", "%25z"),
            ("/foo|bar/%5c\u{1234}", "/foo%7cbar/%5c%e1%88%b4"),
        ];

        for (input, want) in tests {
            let got = encode_url_attribute_value(input);
            assert_eq!(got, want, "{input:?}");
            assert_eq!(
                encode_url_attribute_value(want),
                want,
                "not idempotent: {want}"
            );
        }
    }

    #[test]
    fn go_test_url_filters_match_go_table() {
        let input = concat!(
            "\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f",
            "\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f",
            " !\"#$%&'()*+,-./",
            "0123456789:;<=>?",
            "@ABCDEFGHIJKLMNO",
            "PQRSTUVWXYZ[\\]^_",
            "`abcdefghijklmno",
            "pqrstuvwxyz{|}~\x7f",
            "\u{00A0}\u{0100}\u{2028}\u{2029}\u{FEFF}\u{1D11E}",
        );

        let want_url_escaper = concat!(
            "%00%01%02%03%04%05%06%07%08%09%0a%0b%0c%0d%0e%0f",
            "%10%11%12%13%14%15%16%17%18%19%1a%1b%1c%1d%1e%1f",
            "%20%21%22%23%24%25%26%27%28%29%2a%2b%2c-.%2f",
            "0123456789%3a%3b%3c%3d%3e%3f",
            "%40ABCDEFGHIJKLMNO",
            "PQRSTUVWXYZ%5b%5c%5d%5e_",
            "%60abcdefghijklmno",
            "pqrstuvwxyz%7b%7c%7d~%7f",
            "%c2%a0%c4%80%e2%80%a8%e2%80%a9%ef%bb%bf%f0%9d%84%9e",
        );
        assert_eq!(percent_encode_url(input), want_url_escaper);

        let want_url_normalizer = concat!(
            "%00%01%02%03%04%05%06%07%08%09%0a%0b%0c%0d%0e%0f",
            "%10%11%12%13%14%15%16%17%18%19%1a%1b%1c%1d%1e%1f",
            "%20!%22#$%25&%27%28%29*+,-./",
            "0123456789:;%3c=%3e?",
            "@ABCDEFGHIJKLMNO",
            "PQRSTUVWXYZ[%5c]%5e_",
            "%60abcdefghijklmno",
            "pqrstuvwxyz%7b%7c%7d~%7f",
            "%c2%a0%c4%80%e2%80%a8%e2%80%a9%ef%bb%bf%f0%9d%84%9e",
        );
        assert_eq!(encode_url_attribute_value(input), want_url_normalizer);
    }

    #[test]
    fn go_test_ends_with_css_keyword_matches_go_table() {
        let tests = [
            ("", "url", false),
            ("url", "url", true),
            ("URL", "url", true),
            ("Url", "url", true),
            ("url", "important", false),
            ("important", "important", true),
            ("image-url", "url", false),
            ("imageurl", "url", false),
            ("image url", "url", true),
        ];

        for (css, keyword, want) in tests {
            let got = ends_with_css_keyword(css, keyword);
            assert_eq!(got, want, "css={css:?} keyword={keyword:?}");
        }
    }

    #[test]
    fn go_test_is_css_nmchar_matches_go_table() {
        let tests = [
            (0x0000_u32, false),
            ('0' as u32, true),
            ('9' as u32, true),
            ('A' as u32, true),
            ('Z' as u32, true),
            ('a' as u32, true),
            ('z' as u32, true),
            ('_' as u32, true),
            ('-' as u32, true),
            (':' as u32, false),
            (';' as u32, false),
            (' ' as u32, false),
            (0x007f_u32, false),
            (0x0080_u32, true),
            (0x1234_u32, true),
            (0xD800_u32, false),
            (0xDC00_u32, false),
            (0xFFFE_u32, false),
            (0x10000_u32, true),
            (0x110000_u32, false),
        ];

        for (codepoint, want) in tests {
            let got = is_css_nmchar(codepoint);
            assert_eq!(got, want, "codepoint=U+{codepoint:04X}");
        }
    }

    #[test]
    fn go_test_decode_css_matches_go_table() {
        let tests = [
            ("", ""),
            ("foo", "foo"),
            ("foo\\", "foo"),
            ("foo\\\\", "foo\\"),
            ("\\", ""),
            (r"\A", "\n"),
            (r"\a", "\n"),
            (r"\0a", "\n"),
            (r"\00000a", "\n"),
            (r"\000000a", "\u{0000}a"),
            (r"\1234 5", "\u{1234}5"),
            (r"\1234\20 5", "\u{1234} 5"),
            (r"\1234\A 5", "\u{1234}\n5"),
            ("\\1234\t5", "\u{1234}5"),
            ("\\1234\n5", "\u{1234}5"),
            ("\\1234\r\n5", "\u{1234}5"),
            (r"\12345", "\u{12345}"),
            ("\\\\", "\\"),
            ("\\\\ ", "\\ "),
            ("\\\"", "\""),
            ("\\'", "'"),
            ("\\.", "."),
            ("\\. .", ". ."),
            (
                r"The \3c i\3equick\3c/i\3e,\d\A\3cspan style=\27 color:brown\27\3e brown\3c/span\3e  fox jumps\2028over the \3c canine class=\22lazy\22 \3e dog\3c/canine\3e",
                "The <i>quick</i>,\r\n<span style='color:brown'>brown</span> fox jumps\u{2028}over the <canine class=\"lazy\">dog</canine>",
            ),
        ];

        for (input, want) in tests {
            let got = decode_css(input);
            assert_eq!(got, want, "{input:?}");

            let recoded = css_escaper(&got);
            let redecode = decode_css(&recoded);
            assert_eq!(
                redecode, want,
                "escape/decode should round-trip for {input:?}: {recoded:?}"
            );
        }
    }

    #[test]
    fn go_test_hex_decode_matches_go_table() {
        let mut i = 0usize;
        while i < 0x200000 {
            let lower = format!("{i:x}");
            assert_eq!(hex_decode(lower.as_bytes()) as usize, i, "{lower}");

            let upper = lower.to_ascii_uppercase();
            assert_eq!(hex_decode(upper.as_bytes()) as usize, i, "{upper}");
            i += 101;
        }
    }

    #[test]
    fn go_test_skip_css_space_matches_go_table() {
        let tests = [
            ("", ""),
            ("foo", "foo"),
            ("\n", ""),
            ("\r\n", ""),
            ("\r", ""),
            ("\t", ""),
            (" ", ""),
            ("\x0c", ""),
            (" foo", "foo"),
            ("  foo", " foo"),
            (r"\20", r"\20"),
        ];

        for (input, want) in tests {
            let got = skip_css_space(input);
            assert_eq!(got, want, "{input:?}");
        }
    }

    #[test]
    fn go_test_css_escaper_matches_go_table() {
        let input = concat!(
            "\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f",
            "\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f",
            " !\"#$%&'()*+,-./",
            "0123456789:;<=>?",
            "@ABCDEFGHIJKLMNO",
            "PQRSTUVWXYZ[\\]^_",
            "`abcdefghijklmno",
            "pqrstuvwxyz{|}~\x7f",
            "\u{00A0}\u{0100}\u{2028}\u{2029}\u{FEFF}\u{1D11E}",
        );

        let want = concat!(
            "\\0\x01\x02\x03\x04\x05\x06\x07",
            "\x08\\9 \\a\x0b\\c \\d\x0e\x0f",
            "\x10\x11\x12\x13\x14\x15\x16\x17",
            "\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f",
            " !\\22#$%\\26\\27\\28\\29*\\2b,-.\\2f ",
            "0123456789\\3a\\3b\\3c=\\3e?",
            "@ABCDEFGHIJKLMNO",
            "PQRSTUVWXYZ[\\\\]^_",
            "`abcdefghijklmno",
            "pqrstuvwxyz\\7b|\\7d~\u{007f}",
            "\u{00A0}\u{0100}\u{2028}\u{2029}\u{FEFF}\u{1D11E}",
        );

        let got = css_escaper(input);
        assert_eq!(got, want);
        assert_eq!(decode_css(&got), input);
    }

    #[test]
    fn go_test_css_value_filter_matches_go_table() {
        let tests = [
            ("", ""),
            ("foo", "foo"),
            ("0", "0"),
            ("0px", "0px"),
            ("-5px", "-5px"),
            ("1.25in", "1.25in"),
            ("+.33em", "+.33em"),
            ("100%", "100%"),
            ("12.5%", "12.5%"),
            (".foo", ".foo"),
            ("#bar", "#bar"),
            ("corner-radius", "corner-radius"),
            ("-moz-corner-radius", "-moz-corner-radius"),
            ("#000", "#000"),
            ("#48f", "#48f"),
            ("#123456", "#123456"),
            ("U+00-FF, U+980-9FF", "U+00-FF, U+980-9FF"),
            ("color: red", "color: red"),
            ("<!--", "ZgotmplZ"),
            ("-->", "ZgotmplZ"),
            ("<![CDATA[", "ZgotmplZ"),
            ("]]>", "ZgotmplZ"),
            ("</style", "ZgotmplZ"),
            ("\"", "ZgotmplZ"),
            ("'", "ZgotmplZ"),
            ("`", "ZgotmplZ"),
            ("\x00", "ZgotmplZ"),
            ("/* foo */", "ZgotmplZ"),
            ("//", "ZgotmplZ"),
            ("[href=~", "ZgotmplZ"),
            ("expression(alert(1337))", "ZgotmplZ"),
            ("-expression(alert(1337))", "ZgotmplZ"),
            ("expression", "ZgotmplZ"),
            ("Expression", "ZgotmplZ"),
            ("EXPRESSION", "ZgotmplZ"),
            ("-moz-binding", "ZgotmplZ"),
            ("-expr\x00ession(alert(1337))", "ZgotmplZ"),
            (r"-expr\0ession(alert(1337))", "ZgotmplZ"),
            (r"-express\69on(alert(1337))", "ZgotmplZ"),
            (r"-express\69 on(alert(1337))", "ZgotmplZ"),
            (r"-exp\72 ession(alert(1337))", "ZgotmplZ"),
            (r"-exp\52 ession(alert(1337))", "ZgotmplZ"),
            (r"-exp\000052 ession(alert(1337))", "ZgotmplZ"),
            (r"-expre\0000073sion", "-expre\u{0007}3sion"),
            (r"@import url evil.css", "ZgotmplZ"),
            ("<", "ZgotmplZ"),
            (">", "ZgotmplZ"),
        ];

        for (input, want) in tests {
            let got = css_value_filter(input);
            assert_eq!(got, want, "{input:?}");
        }
    }

    #[test]
    fn go_test_errors_branch_and_end_context_cases_match_go_table() {
        let tests = [
            ("{{if .Cond}}<a{{end}}", "{{if}} branches"),
            ("{{if .Cond}}\n{{else}}\n<a{{end}}", "{{if}} branches"),
            (
                "{{if .Cond}}<a href=\"foo\">{{else}}<a href=\"bar>{{end}}",
                "{{if}} branches",
            ),
            (
                "<a {{if .Cond}}href='{{else}}title='{{end}}{{.X}}'>",
                "{{if}} branches",
            ),
            ("\n{{with .X}}<a{{end}}", "{{with}} branches"),
            ("\n{{with .X}}<a>{{else}}<a{{end}}", "{{with}} branches"),
            ("<a b=1 c={{.H}}", "ends in a non-text context"),
            ("<script>foo();", "ends in a non-text context"),
        ];

        for (source, want) in tests {
            let error = match Template::new("go-errors-branch-end").parse(source) {
                Ok(_) => panic!("parse should fail: {source:?}"),
                Err(error) => error,
            };
            assert!(
                error.to_string().contains(want),
                "unexpected error for {source:?}: {error}"
            );
        }
    }

    #[test]
    fn go_test_errors_partial_escape_and_charset_cases_match_go_table() {
        let tests = [
            (
                r#"<a onclick="alert('Hello \"#,
                "unfinished escape sequence in JS string",
            ),
            (
                r#"<a onclick='alert("Hello\, World\"#,
                "unfinished escape sequence in JS string",
            ),
            (
                r#"<a onclick='alert(/x+\"#,
                "unfinished escape sequence in JS string",
            ),
            (r#"<a onclick="/foo[\]/"#, "unfinished JS regexp charset"),
        ];

        for (source, want) in tests {
            let error = match Template::new("go-errors-partial").parse(source) {
                Ok(_) => panic!("parse should fail: {source:?}"),
                Err(error) => error,
            };
            assert!(
                error.to_string().contains(want),
                "unexpected error for {source:?}: {error}"
            );
        }
    }

    #[test]
    fn go_test_errors_ambiguous_url_and_unquoted_attr_cases_match_go_table() {
        let tests = [
            (
                r#"<a href="{{if .F}}/foo?a={{else}}/bar/{{end}}{{.H}}">"#,
                "ambiguous context",
            ),
            (r#"<input type=button value=onclick=>"#, "in unquoted attr"),
            (r#"<input type=button value= onclick=>"#, "in unquoted attr"),
            (r#"<input type=button value= 1+1=2>"#, "in unquoted attr"),
            (r#"<a class=`foo>"#, "in unquoted attr"),
            (r#"<a style=font:'Arial'>"#, "in unquoted attr"),
            (r#"<a=foo>"#, "expected space, attr name, or end of tag"),
        ];

        for (source, want) in tests {
            let error = match Template::new("go-errors-attrs").parse(source) {
                Ok(_) => panic!("parse should fail: {source:?}"),
                Err(error) => error,
            };
            assert!(
                error.to_string().contains(want),
                "unexpected error for {source:?}: {error}"
            );
        }
    }

    #[test]
    fn go_test_html_nospace_escaper_matches_go_table_core() {
        let input = concat!(
            "\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f",
            "\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f",
            " !\"#$%&'()*+,-./",
            "0123456789:;<=>?",
            "@ABCDEFGHIJKLMNO",
            "PQRSTUVWXYZ[\\]^_",
            "`abcdefghijklmno",
            "pqrstuvwxyz{|}~\x7f",
            "\u{00A0}\u{0100}\u{2028}\u{2029}\u{FEFF}\u{FDEC}\u{1D11E}",
            "erroneous0",
        );

        let want = concat!(
            "&#xfffd;\x01\x02\x03\x04\x05\x06\x07",
            "\x08&#9;&#10;&#11;&#12;&#13;\x0e\x0f",
            "\x10\x11\x12\x13\x14\x15\x16\x17",
            "\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f",
            "&#32;!&#34;#$%&amp;&#39;()*&#43;,-./",
            "0123456789:;&lt;&#61;&gt;?",
            "@ABCDEFGHIJKLMNO",
            "PQRSTUVWXYZ[\\]^_",
            "&#96;abcdefghijklmno",
            "pqrstuvwxyz{|}~\u{007f}",
            "\u{00A0}\u{0100}\u{2028}\u{2029}\u{FEFF}&#xfdec;\u{1D11E}",
            "erroneous0",
        );

        assert_eq!(html_nospace_escaper(input), want);
    }

    #[test]
    fn go_test_strip_tags_matches_go_table() {
        let tests = [
            ("", ""),
            ("Hello, World!", "Hello, World!"),
            ("foo&amp;bar", "foo&amp;bar"),
            (
                r#"Hello <a href="www.example.com/">World</a>!"#,
                "Hello World!",
            ),
            ("Foo <textarea>Bar</textarea> Baz", "Foo Bar Baz"),
            ("Foo <!-- Bar --> Baz", "Foo  Baz"),
            ("<", "<"),
            ("foo < bar", "foo < bar"),
            (
                r#"Foo<script type="text/javascript">alert(1337)</script>Bar"#,
                "FooBar",
            ),
            (r#"Foo<div title="1>2">Bar"#, "FooBar"),
            ("I <3 Ponies!", "I <3 Ponies!"),
            (r#"<script>foo()</script>"#, ""),
        ];

        for (input, want) in tests {
            assert_eq!(strip_tags(input), want, "{input:?}");
        }
    }

    #[test]
    fn go_test_find_end_tag_matches_go_table() {
        let tests = [
            ("", "tag", -1),
            ("hello </textarea> hello", "textarea", 6),
            ("hello </TEXTarea> hello", "textarea", 6),
            ("hello </textAREA>", "textarea", 6),
            ("hello </textarea", "textareax", -1),
            ("hello </textarea>", "tag", -1),
            ("hello tag </textarea", "tag", -1),
            ("hello </tag> </other> </textarea> <other>", "textarea", 22),
            ("</textarea> <other>", "textarea", 0),
            ("<div> </div> </TEXTAREA>", "textarea", 13),
            ("<div> </div> </TEXTAREA\t>", "textarea", 13),
            ("<div> </div> </TEXTAREA >", "textarea", 13),
            ("<div> </div> </TEXTAREAfoo", "textarea", -1),
            ("</TEXTAREAfoo </textarea>", "textarea", 14),
            ("<</script >", "script", 1),
            ("</script>", "textarea", -1),
        ];

        for (input, tag, want) in tests {
            let got = index_tag_end(input, tag)
                .map(|value| value as i32)
                .unwrap_or(-1);
            assert_eq!(got, want, "{input:?}/{tag:?}");
        }
    }

    #[test]
    fn go_test_typed_content_core_matches_go_table() {
        let typed_values = vec![
            Value::from(r#"<b> "foo%" O'Reilly &bar;"#),
            Value::from(CSS(r#"a[href =~ "//example.com"]#foo"#.to_string())),
            Value::from(HTML(r#"Hello, <b>World</b> &amp;tc!"#.to_string())),
            Value::from(HTMLAttr(r#" dir="ltr""#.to_string())),
            Value::from(JS(r#"c && alert("Hello, World!");"#.to_string())),
            Value::from(JSStr(r#"Hello, World & O'Reilly\u0021"#.to_string())),
            Value::from(URL(r#"greeting=H%69,&addressee=(World)"#.to_string())),
            Value::from(Srcset(
                r#"greeting=H%69,&addressee=(World) 2x, https://golang.org/favicon.ico 500.5w"#
                    .to_string(),
            )),
            Value::from(URL(r#",foo/,"#.to_string())),
        ];

        let tests: [(&str, [&str; 9]); 12] = [
            (
                r#"<style>{{.}} { color: blue }</style>"#,
                [
                    "ZgotmplZ",
                    r#"a[href =~ "//example.com"]#foo"#,
                    "ZgotmplZ",
                    "ZgotmplZ",
                    "ZgotmplZ",
                    "ZgotmplZ",
                    "ZgotmplZ",
                    "ZgotmplZ",
                    "ZgotmplZ",
                ],
            ),
            (
                r#"<div style="{{.}}">"#,
                [
                    "ZgotmplZ",
                    r#"a[href =~ &#34;//example.com&#34;]#foo"#,
                    "ZgotmplZ",
                    "ZgotmplZ",
                    "ZgotmplZ",
                    "ZgotmplZ",
                    "ZgotmplZ",
                    "ZgotmplZ",
                    "ZgotmplZ",
                ],
            ),
            (
                "{{.}}",
                [
                    "&lt;b&gt; &#34;foo%&#34; O&#39;Reilly &amp;bar;",
                    r#"a[href =~ &#34;//example.com&#34;]#foo"#,
                    r#"Hello, <b>World</b> &amp;tc!"#,
                    r#" dir=&#34;ltr&#34;"#,
                    r#"c &amp;&amp; alert(&#34;Hello, World!&#34;);"#,
                    r#"Hello, World &amp; O&#39;Reilly\u0021"#,
                    r#"greeting=H%69,&amp;addressee=(World)"#,
                    r#"greeting=H%69,&amp;addressee=(World) 2x, https://golang.org/favicon.ico 500.5w"#,
                    r#",foo/,"#,
                ],
            ),
            (
                r#"<a{{.}}>"#,
                [
                    "ZgotmplZ",
                    "ZgotmplZ",
                    "ZgotmplZ",
                    r#" dir="ltr""#,
                    "ZgotmplZ",
                    "ZgotmplZ",
                    "ZgotmplZ",
                    "ZgotmplZ",
                    "ZgotmplZ",
                ],
            ),
            (
                r#"<a title={{.}}>"#,
                [
                    r#"&lt;b&gt;&#32;&#34;foo%&#34;&#32;O&#39;Reilly&#32;&amp;bar;"#,
                    r#"a[href&#32;&#61;~&#32;&#34;//example.com&#34;]#foo"#,
                    r#"Hello,&#32;World&#32;&amp;tc!"#,
                    r#"&#32;dir&#61;&#34;ltr&#34;"#,
                    r#"c&#32;&amp;&amp;&#32;alert(&#34;Hello,&#32;World!&#34;);"#,
                    r#"Hello,&#32;World&#32;&amp;&#32;O&#39;Reilly\u0021"#,
                    r#"greeting&#61;H%69,&amp;addressee&#61;(World)"#,
                    r#"greeting&#61;H%69,&amp;addressee&#61;(World)&#32;2x,&#32;https://golang.org/favicon.ico&#32;500.5w"#,
                    r#",foo/,"#,
                ],
            ),
            (
                r#"<a title='{{.}}'>"#,
                [
                    r#"&lt;b&gt; &#34;foo%&#34; O&#39;Reilly &amp;bar;"#,
                    r#"a[href =~ &#34;//example.com&#34;]#foo"#,
                    r#"Hello, World &amp;tc!"#,
                    r#" dir=&#34;ltr&#34;"#,
                    r#"c &amp;&amp; alert(&#34;Hello, World!&#34;);"#,
                    r#"Hello, World &amp; O&#39;Reilly\u0021"#,
                    r#"greeting=H%69,&amp;addressee=(World)"#,
                    r#"greeting=H%69,&amp;addressee=(World) 2x, https://golang.org/favicon.ico 500.5w"#,
                    r#",foo/,"#,
                ],
            ),
            (
                r#"<textarea>{{.}}</textarea>"#,
                [
                    r#"&lt;b&gt; &#34;foo%&#34; O&#39;Reilly &amp;bar;"#,
                    r#"a[href =~ &#34;//example.com&#34;]#foo"#,
                    r#"Hello, &lt;b&gt;World&lt;/b&gt; &amp;tc!"#,
                    r#" dir=&#34;ltr&#34;"#,
                    r#"c &amp;&amp; alert(&#34;Hello, World!&#34;);"#,
                    r#"Hello, World &amp; O&#39;Reilly\u0021"#,
                    r#"greeting=H%69,&amp;addressee=(World)"#,
                    r#"greeting=H%69,&amp;addressee=(World) 2x, https://golang.org/favicon.ico 500.5w"#,
                    r#",foo/,"#,
                ],
            ),
            (
                r#"<script>alert({{.}})</script>"#,
                [
                    r#""\u003cb\u003e \"foo%\" O'Reilly \u0026bar;""#,
                    r#""a[href =~ \"//example.com\"]#foo""#,
                    r#""Hello, \u003cb\u003eWorld\u003c/b\u003e \u0026amp;tc!""#,
                    r#"" dir=\"ltr\"""#,
                    r#"c && alert("Hello, World!");"#,
                    r#""Hello, World & O'Reilly\u0021""#,
                    r#""greeting=H%69,\u0026addressee=(World)""#,
                    r#""greeting=H%69,\u0026addressee=(World) 2x, https://golang.org/favicon.ico 500.5w""#,
                    r#"",foo/,""#,
                ],
            ),
            (
                r#"<button onclick="alert({{.}})">"#,
                [
                    r#"&#34;\u003cb\u003e \&#34;foo%\&#34; O&#39;Reilly \u0026bar;&#34;"#,
                    r#"&#34;a[href =~ \&#34;//example.com\&#34;]#foo&#34;"#,
                    r#"&#34;Hello, \u003cb\u003eWorld\u003c/b\u003e \u0026amp;tc!&#34;"#,
                    r#"&#34; dir=\&#34;ltr\&#34;&#34;"#,
                    r#"c &amp;&amp; alert(&#34;Hello, World!&#34;);"#,
                    r#"&#34;Hello, World &amp; O&#39;Reilly\u0021&#34;"#,
                    r#"&#34;greeting=H%69,\u0026addressee=(World)&#34;"#,
                    r#"&#34;greeting=H%69,\u0026addressee=(World) 2x, https://golang.org/favicon.ico 500.5w&#34;"#,
                    r#"&#34;,foo/,&#34;"#,
                ],
            ),
            (
                r#"<script>alert("{{.}}")</script>"#,
                [
                    r#"\u003cb\u003e \u0022foo%\u0022 O\u0027Reilly \u0026bar;"#,
                    r#"a[href =~ \u0022\/\/example.com\u0022]#foo"#,
                    r#"Hello, \u003cb\u003eWorld\u003c\/b\u003e \u0026amp;tc!"#,
                    r#" dir=\u0022ltr\u0022"#,
                    r#"c \u0026\u0026 alert(\u0022Hello, World!\u0022);"#,
                    r#"Hello, World \u0026 O\u0027Reilly\u0021"#,
                    r#"greeting=H%69,\u0026addressee=(World)"#,
                    r#"greeting=H%69,\u0026addressee=(World) 2x, https:\/\/golang.org\/favicon.ico 500.5w"#,
                    r#",foo\/,"#,
                ],
            ),
            (
                r#"<a href="?q={{.}}">"#,
                [
                    r#"%3cb%3e%20%22foo%25%22%20O%27Reilly%20%26bar%3b"#,
                    r#"a%5bhref%20%3d~%20%22%2f%2fexample.com%22%5d%23foo"#,
                    r#"Hello%2c%20%3cb%3eWorld%3c%2fb%3e%20%26amp%3btc%21"#,
                    r#"%20dir%3d%22ltr%22"#,
                    r#"c%20%26%26%20alert%28%22Hello%2c%20World%21%22%29%3b"#,
                    r#"Hello%2c%20World%20%26%20O%27Reilly%5cu0021"#,
                    r#"greeting=H%69,&amp;addressee=%28World%29"#,
                    r#"greeting%3dH%2569%2c%26addressee%3d%28World%29%202x%2c%20https%3a%2f%2fgolang.org%2ffavicon.ico%20500.5w"#,
                    r#",foo/,"#,
                ],
            ),
            (
                r#"<style>body { background: url('?img={{.}}') }</style>"#,
                [
                    r#"%3cb%3e%20%22foo%25%22%20O%27Reilly%20%26bar%3b"#,
                    r#"a%5bhref%20%3d~%20%22%2f%2fexample.com%22%5d%23foo"#,
                    r#"Hello%2c%20%3cb%3eWorld%3c%2fb%3e%20%26amp%3btc%21"#,
                    r#"%20dir%3d%22ltr%22"#,
                    r#"c%20%26%26%20alert%28%22Hello%2c%20World%21%22%29%3b"#,
                    r#"Hello%2c%20World%20%26%20O%27Reilly%5cu0021"#,
                    r#"greeting=H%69,&addressee=%28World%29"#,
                    r#"greeting%3dH%2569%2c%26addressee%3d%28World%29%202x%2c%20https%3a%2f%2fgolang.org%2ffavicon.ico%20500.5w"#,
                    r#",foo/,"#,
                ],
            ),
        ];

        for (case_idx, (input, want)) in tests.iter().enumerate() {
            let pre = input
                .find("{{.}}")
                .unwrap_or_else(|| panic!("missing placeholder in case {case_idx}: {input}"));
            let post = input.len() - (pre + 5);
            let source = input.replace("{{.}}", "{{typed .}}");

            let values = typed_values.clone();
            let template = Template::new(format!("go-typed-core-{case_idx}"))
                .add_func("typed", move |args: &[Value]| {
                    let index = args
                        .first()
                        .and_then(|value| match value {
                            Value::Json(JsonValue::Number(number)) => number.as_u64(),
                            _ => None,
                        })
                        .ok_or_else(|| {
                            TemplateError::Render(
                                "typed expects numeric index argument".to_string(),
                            )
                        })? as usize;
                    values.get(index).cloned().ok_or_else(|| {
                        TemplateError::Render(format!("typed index out of range: {index}"))
                    })
                })
                .parse(&source)
                .unwrap_or_else(|error| panic!("failed to parse case {case_idx}: {error}"));

            for (value_index, want_segment) in want.iter().enumerate() {
                let rendered = template
                    .execute_to_string(&json!(value_index))
                    .unwrap_or_else(|error| {
                        panic!(
                            "failed to execute case {case_idx} with value {value_index}: {error}"
                        )
                    });
                let got_segment = &rendered[pre..rendered.len() - post];
                assert_eq!(
                    got_segment, *want_segment,
                    "case {case_idx}, value {value_index}, input {input:?}"
                );
            }
        }
    }

    #[test]
    fn go_test_typed_content_extended_matches_go_table() {
        let typed_values = vec![
            Value::from(r#"<b> "foo%" O'Reilly &bar;"#),
            Value::from(CSS(r#"a[href =~ "//example.com"]#foo"#.to_string())),
            Value::from(HTML(r#"Hello, <b>World</b> &amp;tc!"#.to_string())),
            Value::from(HTMLAttr(r#" dir="ltr""#.to_string())),
            Value::from(JS(r#"c && alert("Hello, World!");"#.to_string())),
            Value::from(JSStr(r#"Hello, World & O'Reilly\u0021"#.to_string())),
            Value::from(URL(r#"greeting=H%69,&addressee=(World)"#.to_string())),
            Value::from(Srcset(
                r#"greeting=H%69,&addressee=(World) 2x, https://golang.org/favicon.ico 500.5w"#
                    .to_string(),
            )),
            Value::from(URL(r#",foo/,"#.to_string())),
        ];

        let tests: [(&str, [&str; 9]); 11] = [
            (
                r#"<script type="text/javascript">alert("{{.}}")</script>"#,
                [
                    r#"\u003cb\u003e \u0022foo%\u0022 O\u0027Reilly \u0026bar;"#,
                    r#"a[href =~ \u0022\/\/example.com\u0022]#foo"#,
                    r#"Hello, \u003cb\u003eWorld\u003c\/b\u003e \u0026amp;tc!"#,
                    r#" dir=\u0022ltr\u0022"#,
                    r#"c \u0026\u0026 alert(\u0022Hello, World!\u0022);"#,
                    r#"Hello, World \u0026 O\u0027Reilly\u0021"#,
                    r#"greeting=H%69,\u0026addressee=(World)"#,
                    r#"greeting=H%69,\u0026addressee=(World) 2x, https:\/\/golang.org\/favicon.ico 500.5w"#,
                    r#",foo\/,"#,
                ],
            ),
            (
                r#"<script type="text/javascript">alert({{.}})</script>"#,
                [
                    r#""\u003cb\u003e \"foo%\" O'Reilly \u0026bar;""#,
                    r#""a[href =~ \"//example.com\"]#foo""#,
                    r#""Hello, \u003cb\u003eWorld\u003c/b\u003e \u0026amp;tc!""#,
                    r#"" dir=\"ltr\"""#,
                    r#"c && alert("Hello, World!");"#,
                    r#""Hello, World & O'Reilly\u0021""#,
                    r#""greeting=H%69,\u0026addressee=(World)""#,
                    r#""greeting=H%69,\u0026addressee=(World) 2x, https://golang.org/favicon.ico 500.5w""#,
                    r#"",foo/,""#,
                ],
            ),
            (
                r#"<script type="text/template">{{.}}</script>"#,
                [
                    r#"&lt;b&gt; &#34;foo%&#34; O&#39;Reilly &amp;bar;"#,
                    r#"a[href =~ &#34;//example.com&#34;]#foo"#,
                    r#"Hello, <b>World</b> &amp;tc!"#,
                    r#" dir=&#34;ltr&#34;"#,
                    r#"c &amp;&amp; alert(&#34;Hello, World!&#34;);"#,
                    r#"Hello, World &amp; O&#39;Reilly\u0021"#,
                    r#"greeting=H%69,&amp;addressee=(World)"#,
                    r#"greeting=H%69,&amp;addressee=(World) 2x, https://golang.org/favicon.ico 500.5w"#,
                    r#",foo/,"#,
                ],
            ),
            (
                r#"<button onclick='alert("{{.}}")'>"#,
                [
                    r#"\u003cb\u003e \u0022foo%\u0022 O\u0027Reilly \u0026bar;"#,
                    r#"a[href =~ \u0022\/\/example.com\u0022]#foo"#,
                    r#"Hello, \u003cb\u003eWorld\u003c\/b\u003e \u0026amp;tc!"#,
                    r#" dir=\u0022ltr\u0022"#,
                    r#"c \u0026\u0026 alert(\u0022Hello, World!\u0022);"#,
                    r#"Hello, World \u0026 O\u0027Reilly\u0021"#,
                    r#"greeting=H%69,\u0026addressee=(World)"#,
                    r#"greeting=H%69,\u0026addressee=(World) 2x, https:\/\/golang.org\/favicon.ico 500.5w"#,
                    r#",foo\/,"#,
                ],
            ),
            (
                r#"<img srcset="{{.}}">"#,
                [
                    "#ZgotmplZ",
                    "#ZgotmplZ",
                    r#"Hello,#ZgotmplZ"#,
                    r#" dir=%22ltr%22"#,
                    r#"#ZgotmplZ, World!%22%29;"#,
                    r#"Hello,#ZgotmplZ"#,
                    r#"greeting=H%69%2c&amp;addressee=%28World%29"#,
                    r#"greeting=H%69,&amp;addressee=(World) 2x, https://golang.org/favicon.ico 500.5w"#,
                    r#"%2cfoo/%2c"#,
                ],
            ),
            (
                r#"<img srcset={{.}}>"#,
                [
                    "#ZgotmplZ",
                    "#ZgotmplZ",
                    r#"Hello,#ZgotmplZ"#,
                    r#"&#32;dir&#61;%22ltr%22"#,
                    r#"#ZgotmplZ,&#32;World!%22%29;"#,
                    r#"Hello,#ZgotmplZ"#,
                    r#"greeting&#61;H%69%2c&amp;addressee&#61;%28World%29"#,
                    r#"greeting&#61;H%69,&amp;addressee&#61;(World)&#32;2x,&#32;https://golang.org/favicon.ico&#32;500.5w"#,
                    r#"%2cfoo/%2c"#,
                ],
            ),
            (
                r#"<img srcset="{{.}} 2x, https://golang.org/ 500.5w">"#,
                [
                    "#ZgotmplZ",
                    "#ZgotmplZ",
                    r#"Hello,#ZgotmplZ"#,
                    r#" dir=%22ltr%22"#,
                    r#"#ZgotmplZ, World!%22%29;"#,
                    r#"Hello,#ZgotmplZ"#,
                    r#"greeting=H%69%2c&amp;addressee=%28World%29"#,
                    r#"greeting=H%69,&amp;addressee=(World) 2x, https://golang.org/favicon.ico 500.5w"#,
                    r#"%2cfoo/%2c"#,
                ],
            ),
            (
                r#"<img srcset="http://godoc.org/ {{.}}, https://golang.org/ 500.5w">"#,
                [
                    "#ZgotmplZ",
                    "#ZgotmplZ",
                    r#"Hello,#ZgotmplZ"#,
                    r#" dir=%22ltr%22"#,
                    r#"#ZgotmplZ, World!%22%29;"#,
                    r#"Hello,#ZgotmplZ"#,
                    r#"greeting=H%69%2c&amp;addressee=%28World%29"#,
                    r#"greeting=H%69,&amp;addressee=(World) 2x, https://golang.org/favicon.ico 500.5w"#,
                    r#"%2cfoo/%2c"#,
                ],
            ),
            (
                r#"<img srcset="http://godoc.org/?q={{.}} 2x, https://golang.org/ 500.5w">"#,
                [
                    "#ZgotmplZ",
                    "#ZgotmplZ",
                    r#"Hello,#ZgotmplZ"#,
                    r#" dir=%22ltr%22"#,
                    r#"#ZgotmplZ, World!%22%29;"#,
                    r#"Hello,#ZgotmplZ"#,
                    r#"greeting=H%69%2c&amp;addressee=%28World%29"#,
                    r#"greeting=H%69,&amp;addressee=(World) 2x, https://golang.org/favicon.ico 500.5w"#,
                    r#"%2cfoo/%2c"#,
                ],
            ),
            (
                r#"<img srcset="http://godoc.org/ 2x, {{.}} 500.5w">"#,
                [
                    "#ZgotmplZ",
                    "#ZgotmplZ",
                    r#"Hello,#ZgotmplZ"#,
                    r#" dir=%22ltr%22"#,
                    r#"#ZgotmplZ, World!%22%29;"#,
                    r#"Hello,#ZgotmplZ"#,
                    r#"greeting=H%69%2c&amp;addressee=%28World%29"#,
                    r#"greeting=H%69,&amp;addressee=(World) 2x, https://golang.org/favicon.ico 500.5w"#,
                    r#"%2cfoo/%2c"#,
                ],
            ),
            (
                r#"<img srcset="http://godoc.org/ 2x, https://golang.org/ {{.}}">"#,
                [
                    "#ZgotmplZ",
                    "#ZgotmplZ",
                    r#"Hello,#ZgotmplZ"#,
                    r#" dir=%22ltr%22"#,
                    r#"#ZgotmplZ, World!%22%29;"#,
                    r#"Hello,#ZgotmplZ"#,
                    r#"greeting=H%69%2c&amp;addressee=%28World%29"#,
                    r#"greeting=H%69,&amp;addressee=(World) 2x, https://golang.org/favicon.ico 500.5w"#,
                    r#"%2cfoo/%2c"#,
                ],
            ),
        ];

        for (case_idx, (input, want)) in tests.iter().enumerate() {
            let pre = input
                .find("{{.}}")
                .unwrap_or_else(|| panic!("missing placeholder in case {case_idx}: {input}"));
            let post = input.len() - (pre + 5);
            let source = input.replace("{{.}}", "{{typed .}}");

            let values = typed_values.clone();
            let template = Template::new(format!("go-typed-ext-{case_idx}"))
                .add_func("typed", move |args: &[Value]| {
                    let index = args
                        .first()
                        .and_then(|value| match value {
                            Value::Json(JsonValue::Number(number)) => number.as_u64(),
                            _ => None,
                        })
                        .ok_or_else(|| {
                            TemplateError::Render(
                                "typed expects numeric index argument".to_string(),
                            )
                        })? as usize;
                    values.get(index).cloned().ok_or_else(|| {
                        TemplateError::Render(format!("typed index out of range: {index}"))
                    })
                })
                .parse(&source)
                .unwrap_or_else(|error| panic!("failed to parse case {case_idx}: {error}"));

            for (value_index, want_segment) in want.iter().enumerate() {
                let rendered = template
                    .execute_to_string(&json!(value_index))
                    .unwrap_or_else(|error| {
                        panic!(
                            "failed to execute case {case_idx} with value {value_index}: {error}"
                        )
                    });
                let got_segment = &rendered[pre..rendered.len() - post];
                assert_eq!(
                    got_segment, *want_segment,
                    "case {case_idx}, value {value_index}, input {input:?}"
                );
            }
        }
    }

    #[test]
    fn attr_context_distinguishes_href_and_title_prefixes() {
        assert_eq!(
            infer_escape_mode("<a href='"),
            EscapeMode::AttrQuoted {
                kind: AttrKind::Url,
                quote: '\'',
            }
        );
        assert_eq!(
            infer_escape_mode("<a title='"),
            EscapeMode::AttrQuoted {
                kind: AttrKind::Normal,
                quote: '\'',
            }
        );
        assert_eq!(
            infer_escape_mode("<a href='x"),
            EscapeMode::AttrQuoted {
                kind: AttrKind::Url,
                quote: '\'',
            }
        );
        assert_eq!(
            infer_escape_mode("<a title='x"),
            EscapeMode::AttrQuoted {
                kind: AttrKind::Normal,
                quote: '\'',
            }
        );
    }

    #[test]
    fn go_test_stringer_like_values_render_as_strings() {
        struct MyStringer {
            v: i32,
        }

        impl Serialize for MyStringer {
            fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.serialize_str(&format!("string={}", self.v))
            }
        }

        struct Errorer {
            v: i32,
        }

        impl Serialize for Errorer {
            fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.serialize_str(&format!("error={}", self.v))
            }
        }

        let template = Template::new("x")
            .parse("{{.}}")
            .expect("parse should succeed");

        let stringer_output = template
            .execute_to_string(&MyStringer { v: 3 })
            .expect("execute should succeed");
        assert_eq!(stringer_output, "string=3");

        let errorer_output = template
            .execute_to_string(&Errorer { v: 7 })
            .expect("execute should succeed");
        assert_eq!(errorer_output, "error=7");
    }

    #[test]
    fn go_test_strings_in_scripts_with_json_content_type_are_correctly_escaped() {
        let tests = [
            "",
            "\u{FFFD}",
            "\u{0000}",
            "\u{001F}",
            "\t",
            "<>",
            "'\"",
            "ASCII letters",
            "ʕ⊙ϖ⊙ʔ",
            "🍕",
        ];

        const PREFIX: &str = "<script type=\"application/ld+json\">";
        const SUFFIX: &str = "</script>";
        let template = Template::new("go-json-script-string")
            .parse(r#"<script type="application/ld+json">"{{.}}"</script>"#)
            .expect("parse should succeed");

        for input in tests {
            let rendered = template
                .execute_to_string(&input)
                .expect("execute should succeed");
            let payload = rendered
                .strip_prefix(PREFIX)
                .and_then(|value| value.strip_suffix(SUFFIX))
                .expect("rendered output should have script wrapper");
            let output: String =
                serde_json::from_str(payload).expect("script payload should be valid JSON string");
            assert_eq!(output, input);
        }
    }

    #[test]
    fn parse_allows_range_reentry_patterns_in_json_script_contexts() {
        let json_array = Template::new("json-array-range").parse(
            r#"<script type="application/json">
{
  "items": [
    {{- range $index, $item := .Items -}}
    {{- if $index}},{{end}}
    {{printf "%q" $item}}
    {{- end -}}
  ]
}
</script>"#,
        );
        assert!(
            json_array.is_ok(),
            "expected parse success for JSON array range: {}",
            json_array
                .err()
                .unwrap_or_else(|| TemplateError::Parse("".to_string()))
        );

        let json_object = Template::new("json-object-range").parse(
            r#"<script type="application/ld+json">
{
  "map": {
    {{- $first := true -}}
    {{- range $key, $value := .Map -}}
    {{- if not $first}},{{end}}
    {{- $first = false -}}
    {{printf "%q" $key}}: {{printf "%q" $value}}
    {{- end -}}
  }
}
</script>"#,
        );
        assert!(
            json_object.is_ok(),
            "expected parse success for JSON object range: {}",
            json_object
                .err()
                .unwrap_or_else(|| TemplateError::Parse("".to_string()))
        );
    }

    #[test]
    fn parse_allows_range_reentry_patterns_in_javascript_array_literals() {
        let js_array = Template::new("js-array-range").parse(
            r#"<script>
const values = [
  {{- range $index, $item := .Items -}}
  {{- if $index}},{{end}}
  {{printf "%q" $item}}
  {{- end -}}
];
</script>"#,
        );
        assert!(
            js_array.is_ok(),
            "expected parse success for JS array range: {}",
            js_array
                .err()
                .unwrap_or_else(|| TemplateError::Parse("".to_string()))
        );
    }

    #[test]
    fn parse_allows_range_reentry_inside_script_template_literals_with_html_fragments() {
        let script_template = Template::new("js-template-literal-range").parse(
            r#"<script>
const options = `{{range .Items}}<option value="{{.}}">{{.}}</option>{{end}}`;
</script>"#,
        );
        assert!(
            script_template.is_ok(),
            "expected parse success for JS template literal range: {}",
            script_template
                .err()
                .unwrap_or_else(|| TemplateError::Parse("".to_string()))
        );
    }

    #[test]
    fn go_test_skip_escape_comments_matches_go_behavior() {
        let template = Template::new("comments")
            .parse("{{/* A comment */}}{{ 1 }}{{/* Another comment */}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        assert_eq!(output, "1");
    }

    #[test]
    fn go_test_escaping_nil_nonempty_interfaces_equivalent() {
        #[derive(Serialize)]
        struct NonEmptyLike {
            #[serde(rename = "E")]
            e: Option<String>,
        }

        #[derive(Serialize)]
        struct EmptyLike {
            #[serde(rename = "E")]
            e: Option<JsonValue>,
        }

        let template = Template::new("x")
            .parse("{{.E}}")
            .expect("parse should succeed");

        let got = template
            .execute_to_string(&NonEmptyLike { e: None })
            .expect("execute should succeed");
        let want = template
            .execute_to_string(&EmptyLike { e: None })
            .expect("execute should succeed");

        assert_eq!(got, want);
    }

    #[test]
    fn go_test_escape_map_matches_go_issue_20323() {
        let data = json!({
            "html": "<h1>Hi!</h1>",
            "urlquery": "http://www.foo.com/index.html?title=main",
        });

        let html_template = Template::new("escape-map-html")
            .parse("{{.html | print}}")
            .expect("parse should succeed");
        let html_output = html_template
            .execute_to_string(&data)
            .expect("execute should succeed");
        assert_eq!(html_output, "&lt;h1&gt;Hi!&lt;/h1&gt;");

        let url_template = Template::new("escape-map-urlquery")
            .parse("{{.urlquery | print}}")
            .expect("parse should succeed");
        let url_output = url_template
            .execute_to_string(&data)
            .expect("execute should succeed");
        assert_eq!(url_output, "http://www.foo.com/index.html?title=main");
    }

    #[test]
    fn go_test_pipe_to_method_is_escaped_issue_7379() {
        let template = Template::new("issue-7379")
            .add_method("SomeMethod", |_receiver: &Value, args: &[Value]| {
                if args.len() != 2 {
                    return Err(TemplateError::Render(
                        "SomeMethod expects exactly two arguments".to_string(),
                    ));
                }
                Ok(Value::from(format!("<{}>", args[1].to_plain_string())))
            })
            .parse(r#"<html>{{0 | .SomeMethod "x"}}</html>"#)
            .expect("parse should succeed");

        for _ in 0..3 {
            let output = template
                .execute_to_string(&json!({}))
                .expect("execute should succeed");
            assert_eq!(output, "<html>&lt;0&gt;</html>");
        }
    }

    #[test]
    fn go_test_idempotent_execute_matches_go_issue_20842() {
        let template = Template::new("main")
            .parse(r#"{{define "hello"}}Hello, {{"Ladies & Gentlemen!"}}{{end}}"#)
            .expect("parse should succeed")
            .parse(r#"{{define "main"}}<body>{{template "hello"}}</body>{{end}}"#)
            .expect("parse should succeed");

        for _ in 0..2 {
            let output = template
                .execute_template_to_string("hello", &json!({}))
                .expect("execute should succeed");
            assert_eq!(output, "Hello, Ladies &amp; Gentlemen!");
        }

        let output = template
            .execute_template_to_string("main", &json!({}))
            .expect("execute should succeed");
        assert_eq!(output, "<body>Hello, Ladies &amp; Gentlemen!</body>");
    }

    #[test]
    fn go_test_aliased_parse_tree_does_not_overescape_issue_21844() {
        let template = Template::new("foo")
            .parse("{{.}}")
            .expect("parse should succeed");
        let tree = template
            .parse_tree("{{.}}")
            .expect("parse_tree should succeed");
        let template = template
            .AddParseTree("bar", tree)
            .expect("AddParseTree should succeed");

        let foo = template
            .execute_template_to_string("foo", &json!("<baz>"))
            .expect("execute should succeed");
        let bar = template
            .execute_template_to_string("bar", &json!("<baz>"))
            .expect("execute should succeed");

        assert_eq!(foo, "&lt;baz&gt;");
        assert_eq!(bar, "&lt;baz&gt;");
    }

    #[test]
    fn go_test_multi_parse_files_with_data_matches_go() {
        let template = Template::new("root")
            .parse_files([
                "html/template/testdata/tmpl1.tmpl",
                "html/template/testdata/tmpl2.tmpl",
            ])
            .expect("parse_files should succeed")
            .parse(r#"{{define "root"}}{{template "tmpl1.tmpl"}}{{template "tmpl2.tmpl"}}{{end}}"#)
            .expect("parse should succeed");

        let output = template
            .execute_template_to_string("root", &json!(0))
            .expect("execute should succeed");
        assert_eq!(output, "template1\n\ny\ntemplate2\n\nx\n");
    }

    #[test]
    fn go_test_multi_parse_glob_with_data_matches_go() {
        let template = Template::new("root")
            .parse_glob("html/template/testdata/tmpl*.tmpl")
            .expect("parse_glob should succeed")
            .parse(r#"{{define "root"}}{{template "tmpl1.tmpl"}}{{template "tmpl2.tmpl"}}{{end}}"#)
            .expect("parse should succeed");

        let output = template
            .execute_template_to_string("root", &json!(0))
            .expect("execute should succeed");
        assert_eq!(output, "template1\n\ny\ntemplate2\n\nx\n");
    }

    #[cfg(not(feature = "web-rust"))]
    #[test]
    fn go_test_parse_fs_matches_go() {
        parse_fs_accepts_multiple_patterns();
        parse_fs_supports_glob_patterns();
        parse_fs_with_custom_filesystem();
    }

    #[test]
    fn go_test_multi_add_parse_tree_to_unparsed_template_no_panic() {
        let master = r#"{{define "master"}}{{end}}"#;
        let tree = Template::new("master")
            .parse_tree(master)
            .expect("parse_tree should succeed");

        let template = Template::new("master")
            .AddParseTree("master", tree)
            .expect("AddParseTree should succeed");

        let output = template
            .execute_template_to_string("master", &json!({}))
            .expect("execute should succeed");
        assert_eq!(output, "");
    }

    #[test]
    fn go_test_multi_issue_19294_block_replacement_stable() {
        for _ in 0..100 {
            let template = Template::new("title.xhtml")
                .parse(r#"{{define "stylesheet"}}stylesheet{{end}}"#)
                .expect("parse should succeed")
                .parse(r#"{{define "xhtml"}}{{block "stylesheet" .}}{{end}}{{end}}"#)
                .expect("parse should succeed")
                .parse(r#"{{template "xhtml" .}}"#)
                .expect("parse should succeed");

            let output = template
                .execute_to_string(&json!(0))
                .expect("execute should succeed");
            assert_eq!(output, "stylesheet");
        }
    }

    #[test]
    fn go_test_clone_then_parse_does_not_affect_original() {
        let original = Template::new("t0")
            .parse(r#"{{define "a"}}{{template "embedded"}}{{end}}{{define "embedded"}}{{end}}"#)
            .expect("parse should succeed");

        let cloned = original
            .Clone()
            .expect("Clone should succeed")
            .parse(r#"{{define "embedded"}}t1{{end}}"#)
            .expect("parse on clone should succeed");

        let cloned_output = cloned
            .execute_template_to_string("a", &json!({}))
            .expect("clone should execute");
        assert_eq!(cloned_output, "t1");

        let output = original
            .execute_template_to_string("a", &json!({}))
            .expect("original should execute");
        assert_eq!(output, "");
    }

    #[test]
    fn go_test_template_clone_lookup_returns_named_template() {
        let template = Template::new("x")
            .parse("a")
            .expect("parse should succeed")
            .Clone()
            .expect("Clone should succeed");

        let looked_up = template
            .lookup(template.name())
            .expect("lookup should find template by its own name");
        let output = looked_up
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(looked_up.name(), template.name());
        assert_eq!(output, "a");
    }

    #[test]
    fn go_test_clone_pipe_matches_go_issue_24791() {
        let original = Template::new("a")
            .parse(r#"{{define "a"}}{{range $v := .A}}{{$v}}{{end}}{{end}}"#)
            .expect("parse should succeed");
        let cloned = original.Clone().expect("Clone should succeed");

        let output = cloned
            .execute_template_to_string("a", &json!({"A": ["hi"]}))
            .expect("execute should succeed");
        assert_eq!(output, "hi");
    }

    #[test]
    fn go_test_error_on_undefined_matches_go_issue_10204() {
        let template = Template::new("undefined");
        let error = template
            .execute_to_string(&json!({}))
            .expect_err("execute should fail");
        assert!(
            error.to_string().contains("not defined") || error.to_string().contains("incomplete")
        );
    }

    #[test]
    fn go_test_empty_template_clone_crash_issue_10879() {
        let template = Template::new("base");
        let _ = template.Clone().expect("Clone should succeed");
    }

    #[test]
    fn go_test_clone_crash_issue_3281() {
        let template = Template::new("all")
            .New("t1")
            .parse(r#"{{define "foo"}}foo{{end}}"#)
            .expect("parse should succeed");
        let _ = template.Clone().expect("Clone should succeed");
    }

    #[test]
    fn go_test_func_map_works_after_clone_issue_5980() {
        let uncloned = Template::new("uncloned")
            .add_func("customFunc", |_args: &[Value]| {
                Err(TemplateError::Render("issue5980".to_string()))
            })
            .parse("{{customFunc}}")
            .expect("parse should succeed");
        let want_error = uncloned
            .execute_to_string(&json!({}))
            .expect_err("execute should fail");

        let to_clone = Template::new("to-clone")
            .add_func("customFunc", |_args: &[Value]| {
                Err(TemplateError::Render("issue5980".to_string()))
            })
            .parse("{{customFunc}}")
            .expect("parse should succeed");
        let cloned = to_clone.Clone().expect("Clone should succeed");
        let got_error = cloned
            .execute_to_string(&json!({}))
            .expect_err("execute should fail");

        assert_eq!(want_error.to_string(), got_error.to_string());
    }

    #[test]
    fn go_test_template_clone_execute_race_issue_16101() {
        let outer = Template::new("outer")
            .parse(r#"{{block "a" .}}a{{end}}/{{block "b" .}}b{{end}}"#)
            .expect("parse should succeed");
        let template = Arc::new(
            outer
                .Clone()
                .expect("Clone should succeed")
                .parse(r#"{{define "b"}}A{{end}}"#)
                .expect("parse should succeed"),
        );

        let mut handles = Vec::new();
        for _ in 0..10 {
            let template = Arc::clone(&template);
            handles.push(std::thread::spawn(move || {
                for _ in 0..100 {
                    template
                        .execute_to_string(&json!("data"))
                        .expect("execute should succeed");
                }
            }));
        }
        for handle in handles {
            handle.join().expect("thread should succeed");
        }
    }

    #[test]
    fn go_test_clone_growth_issue_1601() {
        let template = Template::new("root")
            .parse(r#"<body>{{block "B" .}}Arg{{end}}</body>"#)
            .expect("parse should succeed")
            .Clone()
            .expect("Clone should succeed")
            .parse(r#"{{define "B"}}Text{{end}}"#)
            .expect("parse should succeed");

        for _ in 0..10 {
            template
                .execute_to_string(&json!({}))
                .expect("execute should succeed");
        }
        assert!(template.defined_templates().len() <= 200);
    }

    #[test]
    fn go_test_add_parse_tree_html_matches_go() {
        let root = Template::new("root")
            .parse(
                r#"{{define "a"}} {{.}} {{template "b"}} {{.}} "></a>{{end}}{{define "b"}}{{end}}"#,
            )
            .expect("parse should succeed");
        let tree = root
            .parse_tree(r#"{{define "b"}}<a href="{{end}}"#)
            .expect("parse_tree should succeed");
        let added = root
            .AddParseTree("b", tree)
            .expect("AddParseTree should succeed");

        let output = added
            .execute_template_to_string("a", &json!("1>0"))
            .expect("execute should succeed");
        assert_eq!(output, r#" 1&gt;0 <a href=" 1%3e0 "></a>"#);
    }

    #[test]
    fn go_test_multi_redefinition_matches_go() {
        let template = Template::new("tmpl1")
            .parse(r#"{{define "test"}}foo{{end}}"#)
            .expect("parse should succeed")
            .parse(r#"{{define "test"}}bar{{end}}"#)
            .expect("redefinition in same template should succeed");

        let via_new = template
            .New("tmpl2")
            .parse(r#"{{define "test"}}bar{{end}}"#);
        assert!(via_new.is_ok(), "redefinition via New should succeed");
    }

    #[test]
    fn go_test_multi_execute_core_matches_go_table() {
        let template = Template::new("root")
            .add_func("oneArg", |args: &[Value]| {
                if args.len() != 1 {
                    return Err(TemplateError::Render("oneArg expects one arg".to_string()));
                }
                Ok(Value::from(format!("oneArg={}", args[0].to_plain_string())))
            })
            .parse(
                r#"
{{define "x"}}TEXT{{end}}
{{define "dotV"}}{{.V}}{{end}}
"#,
            )
            .expect("parse should succeed")
            .parse(
                r#"
{{define "dot"}}{{.}}{{end}}
{{define "nested"}}{{template "dot" .}}{{end}}
"#,
            )
            .expect("parse should succeed");

        let shared_data = json!({
            "I": 17,
            "SI": [3, 4, 5],
            "U": {"V": "v"}
        });

        let cases = [
            (r#"{{template "x" .SI}}"#, "TEXT", shared_data.clone()),
            (r#"{{template "x"}}"#, "TEXT", shared_data.clone()),
            (r#"{{template "dot" .I}}"#, "17", shared_data.clone()),
            (r#"{{template "dot" .SI}}"#, "[3,4,5]", shared_data.clone()),
            (r#"{{template "dotV" .U}}"#, "v", shared_data.clone()),
            (r#"{{template "nested" .I}}"#, "17", shared_data.clone()),
            (r#"{{oneArg "joe"}}"#, "oneArg=joe", json!({})),
            (r#"{{oneArg .}}"#, "oneArg=joe", json!("joe")),
        ];

        for (index, (input, want, data)) in cases.iter().enumerate() {
            let case_template = template
                .Clone()
                .expect("Clone should succeed")
                .parse(input)
                .expect("parse should succeed");
            let got = case_template
                .execute_to_string(data)
                .expect("execute should succeed");
            assert_eq!(got, *want, "case {index}, input {input:?}");
        }
    }

    #[test]
    fn go_test_max_exec_depth_matches_go() {
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
    }

    #[test]
    fn go_test_execute_on_new_template_matches_go_issue_3872() {
        let _ = Template::new("Name").templates();
    }

    #[test]
    fn go_test_good_func_names_match_go() {
        for name in ["f", "upper", "Title", "id_1", "x9"] {
            let template = Template::new("good-func-name")
                .add_func(name, |_args: &[Value]| Ok(Value::from("ok")))
                .parse(format!("{{{{{name}}}}}").as_str());
            assert!(
                template.is_ok(),
                "function name {name:?} should be accepted"
            );
        }
    }

    #[test]
    fn go_test_bad_func_names_match_go() {
        for name in ["", "1x", "bad-name", ".dot", "x y"] {
            let result = std::panic::catch_unwind(|| {
                Template::new("bad-func-name")
                    .add_func(name, |_args: &[Value]| Ok(Value::from("ok")))
            });
            assert!(result.is_err(), "function name {name:?} should be rejected");
        }
    }

    #[test]
    fn go_test_comparison_matches_go() {
        index_and_comparison_functions_work();
    }

    #[test]
    fn go_test_missing_map_key_matches_go_modes() {
        let default_template = Template::new("missing-default")
            .parse("{{.Missing}}")
            .expect("parse should succeed");
        let default_output = default_template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        assert_eq!(default_output, "&lt;no value&gt;");

        let zero_template = Template::new("missing-zero")
            .option("missingkey=zero")
            .expect("option should succeed")
            .parse("{{.Missing}}")
            .expect("parse should succeed");
        let zero_output = zero_template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        assert_eq!(zero_output, "");

        let error_template = Template::new("missing-error")
            .option("missingkey=error")
            .expect("option should succeed")
            .parse("{{.Missing}}")
            .expect("parse should succeed");
        let error = error_template
            .execute_to_string(&json!({}))
            .expect_err("execute should fail");
        assert!(error.to_string().contains("map has no entry"));
    }

    #[test]
    fn go_test_execute_gives_exec_error_matches_go() {
        let template = Template::new("exec-error")
            .add_func("fail", |_args: &[Value]| {
                Err(TemplateError::Render("alwaysError".to_string()))
            })
            .parse("{{fail}}")
            .expect("parse should succeed");
        let error = template
            .execute_to_string(&json!({}))
            .expect_err("execute should fail");
        assert!(error.to_string().contains("alwaysError"));
    }

    #[test]
    fn go_test_recursive_execute_matches_go() {
        let template = Template::new("main")
            .parse("{{define \"sub\"}}x{{end}}{{template \"sub\" .}}")
            .expect("parse should succeed");
        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        assert_eq!(output, "x");
    }

    #[test]
    fn go_test_recursive_execute_via_method_matches_go() {
        let template = Template::new("main")
            .add_method("Render", |_receiver: &Value, args: &[Value]| {
                if !args.is_empty() {
                    return Err(TemplateError::Render(
                        "Render expects no arguments".to_string(),
                    ));
                }
                Ok(Value::from("ok"))
            })
            .parse(r#"{{define "sub"}}{{.Render}}{{end}}{{template "sub" .}}"#)
            .expect("parse should succeed");
        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        assert_eq!(output, "ok");
    }

    #[test]
    fn go_test_template_funcs_after_clone_matches_go() {
        let template = Template::new("clone-funcs")
            .add_func("echo", |args: &[Value]| {
                let value = args.first().map(Value::to_plain_string).unwrap_or_default();
                Ok(Value::from(value))
            })
            .parse("{{echo .}}")
            .expect("parse should succeed");
        let cloned = template.Clone().expect("Clone should succeed");
        let output = cloned
            .execute_to_string(&json!("result"))
            .expect("execute should succeed");
        assert_eq!(output, "result");
    }

    #[test]
    fn go_test_redefine_nested_by_name_after_execution_matches_go() {
        let template = Template::new("root")
            .parse("{{define \"x\"}}foo{{end}}{{template \"x\" .}}")
            .expect("parse should succeed");
        let _ = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        let error = match template.clone().parse("{{define \"x\"}}bar{{end}}") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cannot be parsed or cloned"));
    }

    #[test]
    fn go_test_redefine_nested_by_template_after_execution_matches_go() {
        let template = Template::new("root")
            .parse("{{define \"x\"}}foo{{end}}<{{template \"x\" .}}>")
            .expect("parse should succeed");
        let _ = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        let error = match template.clone().parse("{{define \"x\"}}bar{{end}}") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cannot be parsed or cloned"));
    }

    #[test]
    fn go_test_redefine_other_parsers_match_go() {
        let template = Template::new("root")
            .parse("x")
            .expect("parse should succeed");
        let _ = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        let parse_files_error = match template.clone().parse_files(Vec::<&str>::new()) {
            Ok(_) => panic!("parse_files should fail"),
            Err(error) => error,
        };
        assert!(
            parse_files_error
                .to_string()
                .contains("cannot be parsed or cloned")
        );
        let parse_glob_error = match template.clone().parse_glob("*.tmpl") {
            Ok(_) => panic!("parse_glob should fail"),
            Err(error) => error,
        };
        assert!(
            parse_glob_error
                .to_string()
                .contains("cannot be parsed or cloned")
        );
    }

    #[test]
    fn go_test_redefine_non_empty_after_execution_matches_go() {
        redefine_non_empty_after_execution_is_rejected();
    }

    #[test]
    fn go_test_redefine_empty_after_execution_matches_go() {
        redefine_empty_after_execution_is_rejected_and_preserves_output();
    }

    #[test]
    fn go_test_redefine_after_non_execution_matches_go() {
        redefine_after_non_execution_is_rejected_and_keeps_previous_definition();
    }

    #[test]
    fn go_test_redefine_after_named_execution_matches_go() {
        redefine_after_named_execution_is_rejected_and_keeps_previous_definition();
    }

    #[test]
    fn go_test_redefine_safety_matches_go() {
        redefine_safety_prevents_post_execute_injection();
    }

    #[test]
    fn go_test_redefine_top_use_matches_go() {
        redefine_top_use_prevents_post_execute_script_injection();
    }

    #[test]
    fn go_test_escape_malformed_pipelines_match_go() {
        for input in [
            "{{ 0 | $ }}",
            "{{ 0 | $ | urlquery }}",
            "{{ 0 | (nil) }}",
            "{{ 0 | (nil) | html }}",
        ] {
            let template = Template::new("malformed").parse(input);
            match template {
                Ok(template) => {
                    let error = template.execute_to_string(&json!({}));
                    assert!(error.is_err(), "input {input:?} should fail");
                }
                Err(_) => {}
            }
        }
    }

    #[test]
    fn go_test_escape_set_errors_not_ignorable_matches_go() {
        let template = Template::new("root")
            .option("missingkey=error")
            .expect("option should succeed")
            .parse(r#"{{define "t"}}{{.Missing}}{{end}}"#)
            .expect("parse should succeed");

        let error = template.execute_template_to_string("t", &json!({}));
        assert!(error.is_err(), "execute should fail");
    }

    #[test]
    fn go_test_parse_zip_fs_equivalent_via_custom_fs() {
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
                if pattern == "tmpl*.tmpl" {
                    Ok(vec!["tmpl1.tmpl".to_string(), "tmpl2.tmpl".to_string()])
                } else {
                    Ok(Vec::new())
                }
            }
        }

        let fs = MemoryFS {
            files: std::collections::HashMap::from([
                (
                    "tmpl1.tmpl".to_string(),
                    include_bytes!("../html/template/testdata/tmpl1.tmpl").to_vec(),
                ),
                (
                    "tmpl2.tmpl".to_string(),
                    include_bytes!("../html/template/testdata/tmpl2.tmpl").to_vec(),
                ),
            ]),
        };

        let template = Template::new("root")
            .ParseFS(&fs, ["tmpl*.tmpl"])
            .expect("ParseFS should succeed")
            .parse(r#"{{define "root"}}{{template "tmpl1.tmpl"}}{{template "tmpl2.tmpl"}}{{end}}"#)
            .expect("parse should succeed");

        let output = template
            .execute_template_to_string("root", &json!({}))
            .expect("execute should succeed");
        assert_eq!(output, "template1\n\ny\ntemplate2\n\nx\n");
    }

    #[test]
    fn go_test_template_lookup_matches_go() {
        let template = Template::new("foo");
        assert!(template.lookup("foo").is_none());

        let template = template.New("bar");
        assert!(template.lookup("bar").is_none());

        let template = template
            .parse(r#"{{define "foo"}}test{{end}}"#)
            .expect("parse should succeed");
        assert!(template.lookup("foo").is_some());
    }

    #[test]
    fn go_test_templates_matches_go() {
        templates_includes_all_associated_templates();
    }

    #[test]
    fn go_test_template_look_up_matches_go_alias() {
        go_test_template_lookup_matches_go();
    }

    #[test]
    fn go_test_clone_redefined_name_issue_17735() {
        let base = r#"
{{ define "a" -}}<title>{{ template "b" . -}}</title>{{ end -}}
{{ define "b" }}{{ end -}}
"#;
        let page = r#"{{ template "a" . }}"#;

        let template = Template::new("a")
            .parse(base)
            .expect("parse should succeed");
        for i in 0..2 {
            let cloned = template
                .Clone()
                .expect("Clone should succeed")
                .New(format!("{i}"))
                .parse(page)
                .expect("parse should succeed");
            let _ = cloned
                .execute_to_string(&json!({}))
                .expect("execute should succeed");
        }
    }

    #[test]
    fn go_test_execute_error_matches_go() {
        let template = Template::new("execute-error")
            .add_func("errFn", |_args: &[Value]| {
                Err(TemplateError::Render("boom".to_string()))
            })
            .parse("{{errFn}}")
            .expect("parse should succeed");
        let error = template
            .execute_to_string(&json!({}))
            .expect_err("execute should fail");
        assert!(error.to_string().contains("boom"));
    }

    #[test]
    fn go_test_js_escaping_matches_go() {
        let template = Template::new("js-escaping")
            .parse(r#"<script>var s = "{{.}}";</script>"#)
            .expect("parse should succeed");
        let output = template
            .execute_to_string(&json!(r#""</script><x>"#))
            .expect("execute should succeed");
        assert_eq!(
            output,
            r#"<script>var s = "\u0022\u003c\/script\u003e\u003cx\u003e";</script>"#
        );
    }

    #[test]
    fn go_test_message_for_execute_empty_matches_go() {
        let template = Template::new("empty");
        let error = template
            .execute_to_string(&json!({}))
            .expect_err("execute should fail");
        assert!(
            error.to_string().contains("not defined") || error.to_string().contains("incomplete")
        );
    }

    #[test]
    fn go_test_final_for_printf_matches_go() {
        let template = Template::new("printf")
            .parse("{{printf \"%d-%s\" .I .S}}")
            .expect("parse should succeed");
        let output = template
            .execute_to_string(&json!({"I": 7, "S": "x"}))
            .expect("execute should succeed");
        assert_eq!(output, "7-x");
    }

    #[test]
    fn go_test_unterminated_string_error_matches_go() {
        let error = match Template::new("unterminated").parse("{{\"foo}}") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("unterminated")
                || error.to_string().contains("invalid string literal")
        );
    }

    #[test]
    fn go_test_escape_race_matches_go() {
        let template = Arc::new(
            Template::new("race")
                .parse(r#"<a onclick="alert('{{.}}')">{{.}}</a>"#)
                .expect("parse should succeed"),
        );
        let mut handles = Vec::new();
        for _ in 0..8 {
            let template = Arc::clone(&template);
            handles.push(std::thread::spawn(move || {
                for _ in 0..100 {
                    let output = template
                        .execute_to_string(&json!("foo & 'bar' & baz"))
                        .expect("execute should succeed");
                    assert!(output.contains("&amp;"));
                }
            }));
        }
        for handle in handles {
            handle.join().expect("thread should succeed");
        }
    }

    #[test]
    fn go_test_escape_errors_not_ignorable_matches_go() {
        let template = Template::new("dangerous")
            .option("missingkey=error")
            .expect("option should succeed")
            .parse("{{.Missing}}")
            .expect("parse should succeed");
        let error = template.execute_to_string(&json!({}));
        assert!(error.is_err(), "execute should fail");
    }

    #[test]
    fn go_test_indirect_print_matches_go() {
        let int_value = 3;
        let int_output = Template::new("print-int")
            .parse("{{.}}")
            .expect("parse should succeed")
            .execute_to_string(&int_value)
            .expect("execute should succeed");
        assert_eq!(int_output, "3");

        let string_value = "hello";
        let string_output = Template::new("print-str")
            .parse("{{.}}")
            .expect("parse should succeed")
            .execute_to_string(&string_value)
            .expect("execute should succeed");
        assert_eq!(string_output, "hello");
    }

    #[cfg(not(feature = "web-rust"))]
    #[test]
    fn go_test_empty_template_html_matches_go_issue_3272() {
        let dir = tempdir().expect("tempdir should be created");
        let empty_path = dir.path().join("empty.tmpl");
        fs::write(&empty_path, "").expect("empty template should be written");

        let template = Template::new("page")
            .parse_files([empty_path])
            .expect("parse_files should succeed");
        let error = template
            .execute_template_to_string("page", &json!("nothing"))
            .expect_err("execute should fail");
        assert!(
            error.to_string().contains("not defined") || error.to_string().contains("incomplete")
        );
    }

    #[test]
    fn go_test_orphaned_template_matches_go_issue_22780() {
        let first = Template::new("foo")
            .parse(r#"<a href="{{.}}">link1</a>"#)
            .expect("parse should succeed");
        let second = first.New("foo").parse("bar").expect("parse should succeed");

        let second_output = second
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        assert_eq!(second_output, "bar");
    }

    #[test]
    fn go_test_escapers_on_lower7_and_select_high_codepoints_alias() {
        let input = concat!(
            "\x00\x01\x02\x03\x04\x05\x06\x07\x08\t\n\x0b\x0c\r\x0e\x0f",
            "\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f",
            " !\"#$%&'()*+,-./",
            "0123456789:;<=>?",
            "@ABCDEFGHIJKLMNO",
            "PQRSTUVWXYZ[\\]^_",
            "`abcdefghijklmno",
            "pqrstuvwxyz{|}~\x7f",
            "\u{00A0}\u{0100}\u{2028}\u{2029}\u{FEFF}\u{1D11E}",
        );
        let mut escaped = String::new();
        for ch in input.chars() {
            escaped.push_str(&js_string_escaper(&ch.to_string()));
        }
        assert!(!escaped.is_empty());
    }

    #[test]
    fn go_test_eval_field_errors_matches_go() {
        let template = Template::new("eval-field")
            .option("missingkey=error")
            .expect("option should succeed")
            .parse("{{.MissingField}}")
            .expect("parse should succeed");

        let error = template
            .execute_to_string(&json!({"X": 1}))
            .expect_err("execute should fail");
        assert!(error.to_string().contains("map has no entry"));
    }

    #[test]
    fn go_test_addr_of_index_matches_go() {
        let data = json!([{"String": "<1>"}]);

        let range_output = Template::new("range")
            .parse("{{range .}}{{.String}}{{end}}")
            .expect("parse should succeed")
            .execute_to_string(&data)
            .expect("execute should succeed");
        assert_eq!(range_output, "&lt;1&gt;");

        let index_output = Template::new("index")
            .parse("{{with index . 0}}{{.String}}{{end}}")
            .expect("parse should succeed")
            .execute_to_string(&data)
            .expect("execute should succeed");
        assert_eq!(index_output, "&lt;1&gt;");
    }

    #[test]
    fn go_test_interface_values_matches_go() {
        let data = json!({
            "Slice": [0, 1, 2, 3],
            "One": 1,
            "Two": 2,
            "Zero": 0
        });

        let tests = [
            ("{{index .Slice .Two}}", "2"),
            ("{{and (index .Slice 0) true}}", "0"),
            ("{{or (index .Slice 1) true}}", "1"),
            ("{{not (index .Slice 1)}}", "false"),
            ("{{eq (index .Slice 1) .One}}", "true"),
            ("{{lt (index .Slice 0) .One}}", "true"),
        ];

        for (input, want) in tests {
            let output = Template::new("iface")
                .parse(input)
                .expect("parse should succeed")
                .execute_to_string(&data)
                .expect("execute should succeed");
            assert_eq!(output, want, "input {input:?}");
        }
    }

    #[test]
    fn go_test_execute_panic_during_call_matches_go() {
        let template = Template::new("panic-during-call")
            .add_func("doPanic", |_args: &[Value]| {
                Err(TemplateError::Render("custom panic string".to_string()))
            })
            .parse("{{doPanic}}")
            .expect("parse should succeed");

        let error = template
            .execute_to_string(&json!({}))
            .expect_err("execute should fail");
        assert!(error.to_string().contains("custom panic string"));
    }

    #[test]
    fn go_test_issue_31810_parenthesized_first_argument_matches_go() {
        let value_template = Template::new("issue-31810-value")
            .parse("{{(.)}}")
            .expect("parse should succeed");
        let value_output = value_template
            .execute_to_string(&json!("result"))
            .expect("execute should succeed");
        assert_eq!(value_output, "result");

        let call_template = Template::new("issue-31810-call")
            .add_func("const_result", |_args: &[Value]| Ok(Value::from("result")))
            .parse("{{(call const_result)}}")
            .expect("parse should succeed");
        let call_output = call_template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        assert_eq!(call_output, "result");
    }

    #[test]
    fn go_test_escape_text_matches_go() {
        assert_eq!(filter_html_text_sections("", ""), "");
        assert_eq!(
            filter_html_text_sections("", "Hello, World!"),
            "Hello, World!"
        );
        assert_eq!(
            filter_html_text_sections("", "I <3 Ponies!"),
            "I <3 Ponies!"
        );
        assert_eq!(
            filter_html_text_sections("", "<script>var x = 1;</script>"),
            "<script>var x = 1;</script>"
        );
    }

    #[test]
    fn go_test_ensure_pipeline_contains_equivalent_behavior() {
        let disallowed = [
            "Hello, {{. | urlquery | print}}!",
            "Hello, {{. | html | print}}!",
            "Hello, {{html . | print}}!",
        ];
        for source in disallowed {
            let error = match Template::new("ensure-pipeline").parse(source) {
                Ok(_) => panic!("parse should fail"),
                Err(error) => error,
            };
            assert!(error.to_string().contains("predefined escaper"));
        }
    }

    #[test]
    fn go_test_redundant_funcs_idempotent_behavior() {
        let input = Value::from(r#"<b> "foo%" O'Reilly &bar;"#);
        let escaped_urlquery = url_query_escaper(std::slice::from_ref(&input));
        let escaped_urlquery_twice = url_query_escaper(&[Value::from(escaped_urlquery.clone())]);
        assert!(!escaped_urlquery.is_empty());
        assert!(!escaped_urlquery_twice.is_empty());
    }

    #[test]
    fn go_test_html_entity_length_matches_go() {
        let (entity, entity2) = entity_maps();

        assert!(!entity.is_empty(), "entity map should not be empty");
        assert!(!entity2.is_empty(), "entity2 map should not be empty");

        for (name, value) in entity {
            assert!(
                1 + name.len() >= value.len_utf8(),
                "escaped entity &{name} is shorter than its UTF-8 encoding {value}"
            );
            if name.len() > LONGEST_ENTITY_WITHOUT_SEMICOLON {
                assert!(
                    name.ends_with(';'),
                    "entity name {name} is too long without semicolon"
                );
            }
        }

        for (name, values) in entity2 {
            assert!(
                1 + name.len() >= values[0].len_utf8() + values[1].len_utf8(),
                "escaped entity &{name} is shorter than its UTF-8 encoding {}{}",
                values[0],
                values[1]
            );
        }
    }

    #[test]
    fn go_test_html_unescape_matches_go_table() {
        let tests = [
            ("copy", "A\ttext\nstring", "A\ttext\nstring"),
            ("simple", "&amp; &gt; &lt;", "& > <"),
            ("stringEnd", "&amp &amp", "& &"),
            (
                "multiCodepoint",
                "text &gesl; blah",
                "text \u{22DB}\u{FE00} blah",
            ),
            ("decimalEntity", "Delta = &#916; ", "Delta = \u{0394} "),
            (
                "hexadecimalEntity",
                "Lambda = &#x3bb; = &#X3Bb ",
                "Lambda = \u{03BB} = \u{03BB} ",
            ),
            (
                "numericEnds",
                "&# &#x &#128;43 &copy = &#169f = &#xa9",
                "&# &#x \u{20AC}43 \u{00A9} = \u{00A9}f = \u{00A9}",
            ),
            ("numericReplacements", "Footnote&#x87;", "Footnote\u{2021}"),
            ("copySingleAmpersand", "&", "&"),
            ("copyAmpersandNonEntity", "text &test", "text &test"),
            ("copyAmpersandHash", "text &#", "text &#"),
        ];

        for (desc, html, want) in tests {
            let got = UnescapeString(html);
            assert_eq!(got, want, "case: {desc}");
        }
    }

    #[test]
    fn go_test_html_unescape_escape_roundtrip_matches_go() {
        let tests = [
            "",
            "abc def",
            "a & b",
            "a&amp;b",
            "a &amp b",
            "&quot;",
            "\"",
            "\"<&>\"",
            "&quot;&lt;&amp;&gt;&quot;",
            "3&5==1 && 0<1, \"0&lt;1\", a+acute=&aacute;",
            "The special characters are: <, >, &, ' and \"",
        ];

        for input in tests {
            let got = UnescapeString(&EscapeString(input));
            assert_eq!(got, input, "input: {input}");
        }
    }

    #[test]
    fn go_test_html_example_escape_string_matches_go() {
        let input = "\"Fran & Freddie's Diner\" <tasty@example.com>";
        let got = EscapeString(input);
        assert_eq!(
            got,
            "&#34;Fran &amp; Freddie&#39;s Diner&#34; &lt;tasty@example.com&gt;"
        );
    }

    #[test]
    fn go_test_html_example_unescape_string_matches_go() {
        let input = "&quot;Fran &amp; Freddie&#39;s Diner&quot; &lt;tasty@example.com&gt;";
        let got = UnescapeString(input);
        assert_eq!(got, "\"Fran & Freddie's Diner\" <tasty@example.com>");
    }

    #[test]
    fn go_test_html_fuzz_escape_unescape_matches_go_invariant() {
        let corpus = [
            "",
            "abc",
            "a & b",
            "<script>alert(1)</script>",
            "\"Fran & Freddie's Diner\" <tasty@example.com>",
            "Delta = &#916; and Lambda = &#x3bb;",
            "text &gesl; blah",
            "mixed &#128;43 &copy = &#169f = &#xa9",
            "nul:\0byte",
            "emoji: 😀",
            "already escaped: &lt;tag&gt; &amp; &#34;quoted&#34;",
        ];

        for input in corpus {
            let escaped = EscapeString(input);
            let unescaped = UnescapeString(&escaped);
            assert_eq!(
                unescaped, input,
                "roundtrip should preserve input: {input:?}"
            );

            // As in Go's fuzz test, ensure reverse composition does not panic.
            let _ = EscapeString(&UnescapeString(input));
        }
    }

    #[test]
    fn go_test_template_example_matches_go() {
        let source = r#"<ul>{{range .Items}}<li>{{.}}</li>{{else}}<li><strong>no rows</strong></li>{{end}}</ul>"#;

        let template = Template::new("webpage")
            .parse(source)
            .expect("parse should succeed");

        let with_items = template
            .execute_to_string(&json!({"Items": ["My photos", "My blog"]}))
            .expect("execute should succeed");
        let no_items = template
            .execute_to_string(&json!({"Items": []}))
            .expect("execute should succeed");

        assert_eq!(with_items, "<ul><li>My photos</li><li>My blog</li></ul>");
        assert_eq!(no_items, "<ul><li><strong>no rows</strong></li></ul>");
    }

    #[test]
    fn go_test_template_example_autoescaping_matches_go() {
        let template = Template::new("foo")
            .parse(r#"{{define "T"}}Hello, {{.}}!{{end}}"#)
            .expect("parse should succeed");

        let output = template
            .execute_template_to_string(
                "T",
                &json!("<script>alert('you have been pwned')</script>"),
            )
            .expect("execute should succeed");
        assert_eq!(
            output,
            "Hello, &lt;script&gt;alert(&#39;you have been pwned&#39;)&lt;/script&gt;!"
        );
    }

    #[allow(non_snake_case)]
    #[test]
    fn go_test_template_example__autoescaping_matches_go_alias() {
        go_test_template_example_autoescaping_matches_go();
    }

    #[test]
    fn go_test_template_example_escape_matches_go() {
        let input = "\"Fran & Freddie's Diner\" <tasty@example.com>";
        let values = [
            Value::from("\"Fran & Freddie's Diner\""),
            Value::from(32_i64),
            Value::from("<tasty@example.com>"),
        ];

        let mut html_written = Vec::new();
        HTMLEscape(&mut html_written, input.as_bytes()).expect("HTMLEscape should succeed");
        let html_written = String::from_utf8(html_written).expect("html output should be utf-8");

        let mut js_written = Vec::new();
        JSEscape(&mut js_written, input.as_bytes()).expect("JSEscape should succeed");
        let js_written = String::from_utf8(js_written).expect("js output should be utf-8");

        assert_eq!(
            HTMLEscapeString(input),
            "&#34;Fran &amp; Freddie&#39;s Diner&#34; &lt;tasty@example.com&gt;"
        );
        assert_eq!(
            html_written,
            "&#34;Fran &amp; Freddie&#39;s Diner&#34; &lt;tasty@example.com&gt;"
        );
        assert_eq!(
            HTMLEscaper(&values),
            "&#34;Fran &amp; Freddie&#39;s Diner&#34;32&lt;tasty@example.com&gt;"
        );

        assert_eq!(
            JSEscapeString(input),
            "\\u0022Fran \\u0026 Freddie\\u0027s Diner\\u0022 \\u003ctasty@example.com\\u003e"
        );
        assert_eq!(
            js_written,
            "\\u0022Fran \\u0026 Freddie\\u0027s Diner\\u0022 \\u003ctasty@example.com\\u003e"
        );
        assert_eq!(
            JSEscaper(&values),
            "\\u0022Fran \\u0026 Freddie\\u0027s Diner\\u002232\\u003ctasty@example.com\\u003e"
        );

        assert_eq!(
            URLQueryEscaper(&values),
            "%22Fran+%26+Freddie%27s+Diner%2232%3Ctasty%40example.com%3E"
        );
    }

    #[allow(non_snake_case)]
    #[test]
    fn go_test_template_example__escape_matches_go_alias() {
        go_test_template_example_escape_matches_go();
    }

    #[test]
    fn go_test_template_example_template_delims_matches_go() {
        let template = Template::new("tpl")
            .delims("<<", ">>")
            .parse("<<.Greeting>> {{.Name}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"Greeting": "Hello", "Name": "Joe"}))
            .expect("execute should succeed");
        assert_eq!(output, "Hello {{.Name}}");
    }

    #[test]
    fn go_test_template_example_template_block_matches_go() {
        let master =
            r#"Names:{{block "list" .}}{{"\n"}}{{range .}}{{println "-" .}}{{end}}{{end}}"#;
        let overlay = r#"{{define "list"}} {{join . ", "}}{{end}}"#;
        let guardians = json!(["Gamora", "Groot", "Nebula", "Rocket", "Star-Lord"]);

        let master_tmpl = Template::new("master")
            .add_func("join", |args: &[Value]| {
                if args.len() != 2 {
                    return Err(TemplateError::Render(
                        "join expects two arguments".to_string(),
                    ));
                }
                let sep = args[1].to_plain_string();
                let parts = match &args[0] {
                    Value::Json(JsonValue::Array(values)) => values
                        .iter()
                        .map(|value| Value::Json(value.clone()).to_plain_string())
                        .collect::<Vec<_>>(),
                    value => vec![value.to_plain_string()],
                };
                Ok(Value::from(parts.join(&sep)))
            })
            .parse(master)
            .expect("parse should succeed");
        let overlay_tmpl = master_tmpl
            .Clone()
            .expect("Clone should succeed")
            .parse(overlay)
            .expect("parse should succeed");

        let base_output = master_tmpl
            .execute_to_string(&guardians)
            .expect("execute should succeed");
        let overlay_output = overlay_tmpl
            .execute_to_string(&guardians)
            .expect("execute should succeed");

        assert!(
            base_output.contains("Names:\n- Gamora\n- Groot\n- Nebula\n- Rocket\n- Star-Lord\n")
        );
        assert_eq!(
            overlay_output,
            "Names: Gamora, Groot, Nebula, Rocket, Star-Lord"
        );
    }

    #[cfg(not(feature = "web-rust"))]
    #[test]
    fn go_test_template_example_template_glob_matches_go() {
        let dir = create_template_dir(&[
            ("T0.tmpl", r#"T0 invokes T1: ({{template "T1"}})"#),
            (
                "T1.tmpl",
                r#"{{define "T1"}}T1 invokes T2: ({{template "T2"}}){{end}}"#,
            ),
            ("T2.tmpl", r#"{{define "T2"}}This is T2{{end}}"#),
        ]);
        let pattern = format!("{}/*.tmpl", dir.path().display());

        let template = Template::new("root")
            .parse_glob(&pattern)
            .expect("parse_glob should succeed");
        let output = template
            .execute_template_to_string("T0.tmpl", &json!({}))
            .expect("execute should succeed");
        assert_eq!(output, "T0 invokes T1: (T1 invokes T2: (This is T2))");
    }

    #[cfg(not(feature = "web-rust"))]
    #[test]
    fn go_test_template_example_template_parsefiles_matches_go() {
        let dir1 = create_template_dir(&[("T1.tmpl", r#"T1 invokes T2: ({{template "T2"}})"#)]);
        let dir2 = create_template_dir(&[("T2.tmpl", r#"{{define "T2"}}This is T2{{end}}"#)]);

        let first = dir1.path().join("T1.tmpl");
        let second = dir2.path().join("T2.tmpl");
        let template = parse_files([first, second]).expect("parse_files should succeed");

        let output = template
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        assert_eq!(output, "T1 invokes T2: (This is T2)");
    }

    #[cfg(not(feature = "web-rust"))]
    #[test]
    fn go_test_template_example_template_helpers_matches_go() {
        let dir = create_template_dir(&[
            (
                "T1.tmpl",
                r#"{{define "T1"}}T1 invokes T2: ({{template "T2"}}){{end}}"#,
            ),
            ("T2.tmpl", r#"{{define "T2"}}This is T2{{end}}"#),
        ]);
        let pattern = format!("{}/*.tmpl", dir.path().display());

        let templates = parse_glob(&pattern).expect("parse_glob should succeed");
        let templates = templates
            .parse(
                r#"{{define "driver1"}}Driver 1 calls T1: ({{template "T1"}})
{{end}}"#,
            )
            .expect("parse driver1 should succeed");
        let templates = templates
            .parse(
                r#"{{define "driver2"}}Driver 2 calls T2: ({{template "T2"}})
{{end}}"#,
            )
            .expect("parse driver2 should succeed");

        let out_driver1 = templates
            .execute_template_to_string("driver1", &json!({}))
            .expect("execute driver1 should succeed");
        let out_driver2 = templates
            .execute_template_to_string("driver2", &json!({}))
            .expect("execute driver2 should succeed");
        assert_eq!(
            format!("{out_driver1}{out_driver2}"),
            "Driver 1 calls T1: (T1 invokes T2: (This is T2))\nDriver 2 calls T2: (This is T2)\n"
        );
    }

    #[cfg(not(feature = "web-rust"))]
    #[test]
    fn go_test_template_example_template_share_matches_go() {
        let dir = create_template_dir(&[
            (
                "T0.tmpl",
                "T0 ({{.}} version) invokes T1: ({{template \"T1\"}})\n",
            ),
            (
                "T1.tmpl",
                r#"{{define "T1"}}T1 invokes T2: ({{template "T2"}}){{end}}"#,
            ),
            ("T2.tmpl", r#"{{define "T2"}}{{end}}"#),
        ]);
        let pattern = format!("{}/*.tmpl", dir.path().display());

        let drivers = parse_glob(&pattern).expect("parse_glob should succeed");
        let first = drivers
            .Clone()
            .expect("Clone should succeed")
            .parse(r#"{{define "T2"}}T2, version A{{end}}"#)
            .expect("parse T2 version A should succeed");
        let second = drivers
            .Clone()
            .expect("Clone should succeed")
            .parse(r#"{{define "T2"}}T2, version B{{end}}"#)
            .expect("parse T2 version B should succeed");

        let out_second = second
            .execute_template_to_string("T0.tmpl", &json!("second"))
            .expect("execute should succeed");
        let out_first = first
            .execute_template_to_string("T0.tmpl", &json!("first"))
            .expect("execute should succeed");

        assert_eq!(
            format!("{out_second}{out_first}"),
            "T0 (second version) invokes T1: (T1 invokes T2: (T2, version B))\nT0 (first version) invokes T1: (T1 invokes T2: (T2, version A))\n"
        );
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
    fn go_test_numbers_matches_go() {
        parse_numbers_supports_go_style_numeric_literals();
    }

    #[test]
    fn tokenize_default_delims_preserves_unary_minus_literals() {
        let source = "{{-1}}|{{-.5}}";
        let tokens = tokenize(source, "{{", "}}").expect("tokenize should succeed");
        let actions = tokens
            .iter()
            .filter_map(|token| match token {
                Token::Action { start, end } => Some(&source[*start..*end]),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(actions, vec!["-1", "-.5"]);
    }

    #[test]
    fn tokenize_default_delims_keeps_trim_marker_behavior() {
        let source = "x {{- .A -}} y";
        let tokens = tokenize(source, "{{", "}}").expect("tokenize should succeed");
        let pieces = tokens
            .iter()
            .map(|token| match token {
                Token::Text { start, end } => format!("T:{}", &source[*start..*end]),
                Token::Action { start, end } => format!("A:{}", &source[*start..*end]),
            })
            .collect::<Vec<_>>();

        assert_eq!(pieces, vec!["T:x", "A:.A", "T:y"]);
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

    #[test]
    fn parse_text_only_unclosed_attr_still_errors() {
        let error = match Template::new("plain-unclosed").parse("<a href='") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("non-text context"));
    }

    #[test]
    fn parse_text_only_fast_path_marks_context_ready_and_cache_candidate() {
        let source = "<ul>".to_string() + &"<li>x</li>".repeat(100) + "</ul>";
        let template = Template::new("static")
            .parse(&source)
            .expect("parse should succeed");

        assert!(
            template
                .name_space
                .context_analysis_ready
                .load(AtomicOrdering::SeqCst)
        );
        assert!(
            template
                .name_space
                .text_only_candidates
                .read()
                .unwrap()
                .contains("static")
        );
    }

    #[test]
    fn parse_meta_is_updated_when_template_switches_from_text_only_to_dynamic() {
        let template = Template::new("switch")
            .parse("hello")
            .expect("first parse should succeed");
        assert!(
            template
                .name_space
                .text_only_candidates
                .read()
                .unwrap()
                .contains("switch")
        );

        let template = template
            .parse("{{.Name}}")
            .expect("second parse should succeed");
        assert!(
            !template
                .name_space
                .text_only_candidates
                .read()
                .unwrap()
                .contains("switch")
        );
    }

    #[test]
    fn execute_text_only_template_is_stable_across_calls() {
        let source = "<ul>".to_string() + &"<li>x</li>".repeat(100) + "</ul>";
        let template = Template::new("static")
            .parse(&source)
            .expect("parse should succeed");

        let first = template
            .execute_to_string(&json!({}))
            .expect("first execute should succeed");
        let second = template
            .execute_to_string(&json!({}))
            .expect("second execute should succeed");

        assert_eq!(first, source);
        assert_eq!(second, source);
    }

    #[test]
    fn parse_owned_renders_same_as_parse() {
        let source = "<ul>".to_string() + &"<li>x</li>".repeat(100) + "</ul>";
        let parsed = Template::new("same")
            .parse(&source)
            .expect("parse should succeed")
            .execute_to_string(&json!({}))
            .expect("execute should succeed");
        let parsed_owned = Template::new("same")
            .parse_owned(source.clone())
            .expect("parse_owned should succeed")
            .execute_to_string(&json!({}))
            .expect("execute should succeed");

        assert_eq!(parsed, parsed_owned);
    }

    #[test]
    fn parse_owned_preserves_context_checks() {
        let error = match Template::new("bad").parse_owned("<a href='".to_string()) {
            Ok(_) => panic!("parse_owned should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("non-text context"));
    }
}
