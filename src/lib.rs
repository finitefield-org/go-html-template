use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

use serde::Serialize;
use serde_json::Value as JsonValue;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, TemplateError>;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderFlow {
    Normal,
    Break,
    Continue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingKeyMode {
    Default,
    Zero,
    Error,
}

#[derive(Clone)]
pub struct Template {
    name: String,
    templates: HashMap<String, Vec<Node>>,
    funcs: FuncMap,
    methods: MethodMap,
    missing_key_mode: MissingKeyMode,
    left_delim: String,
    right_delim: String,
    executed: Arc<AtomicBool>,
}

impl Template {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            templates: HashMap::new(),
            funcs: builtin_funcs(),
            methods: HashMap::new(),
            missing_key_mode: MissingKeyMode::Default,
            left_delim: "{{".to_string(),
            right_delim: "}}".to_string(),
            executed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn funcs(mut self, funcs: FuncMap) -> Self {
        self.funcs.extend(funcs);
        self
    }

    pub fn add_func<F>(mut self, name: impl Into<String>, function: F) -> Self
    where
        F: Fn(&[Value]) -> Result<Value> + Send + Sync + 'static,
    {
        self.funcs.insert(name.into(), Arc::new(function));
        self
    }

    pub fn methods(mut self, methods: MethodMap) -> Self {
        self.methods.extend(methods);
        self
    }

    pub fn add_method<F>(mut self, name: impl Into<String>, method: F) -> Self
    where
        F: Fn(&Value, &[Value]) -> Result<Value> + Send + Sync + 'static,
    {
        self.methods.insert(name.into(), Arc::new(method));
        self
    }

    pub fn delims(mut self, left: impl Into<String>, right: impl Into<String>) -> Self {
        self.left_delim = left.into();
        self.right_delim = right.into();
        self
    }

    pub fn clone_template(&self) -> Result<Self> {
        if self.left_delim.is_empty() || self.right_delim.is_empty() {
            return Err(TemplateError::Parse(
                "template delimiters must not be empty".to_string(),
            ));
        }
        self.ensure_not_executed()?;

        let mut clone = self.clone();
        clone.executed = Arc::new(AtomicBool::new(false));
        Ok(clone)
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

        self.missing_key_mode = match value {
            "default" | "invalid" => MissingKeyMode::Default,
            "zero" => MissingKeyMode::Zero,
            "error" => MissingKeyMode::Error,
            _ => {
                return Err(TemplateError::Parse(format!(
                    "unsupported missingkey option `{value}`"
                )));
            }
        };
        Ok(())
    }

    pub fn parse(mut self, text: &str) -> Result<Self> {
        self.ensure_not_executed()?;
        let root = self.name.clone();
        self.parse_named(&root, text)?;
        self.reanalyze_contexts()?;
        Ok(self)
    }

    pub fn parse_files<I, P>(mut self, paths: I) -> Result<Self>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        self.ensure_not_executed()?;
        let mut parsed_any = false;
        for path in paths {
            let path = path.as_ref();
            let source = fs::read_to_string(path)?;
            let name = path
                .file_name()
                .and_then(|part| part.to_str())
                .ok_or_else(|| {
                    TemplateError::Parse(format!("invalid template file name: {}", path.display()))
                })?
                .to_string();

            self.parse_named(&name, &source)?;
            if !self.templates.contains_key(&self.name) {
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

    pub fn parse_glob(self, pattern: &str) -> Result<Self> {
        self.ensure_not_executed()?;
        let mut paths = Vec::new();
        for entry in glob::glob(pattern)? {
            paths.push(entry?);
        }
        paths.sort();
        self.parse_files(paths)
    }

    pub fn parse_fs<I, S>(self, patterns: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.ensure_not_executed()?;
        let paths = expand_glob_patterns(patterns)?;
        self.parse_files(paths)
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
        self.executed.store(true, AtomicOrdering::SeqCst);
        let root = Value::from_serializable(data)?;
        let mut rendered = String::new();
        let mut scopes = vec![HashMap::new()];
        let flow = self.render_named(name, &root, &root, &mut scopes, &mut rendered, false)?;
        if !matches!(flow, RenderFlow::Normal) {
            return Err(TemplateError::Render(
                "break/continue action is not inside range".to_string(),
            ));
        }
        writer.write_all(rendered.as_bytes())?;
        Ok(())
    }

    pub fn execute_template_to_string<T: Serialize>(&self, name: &str, data: &T) -> Result<String> {
        self.executed.store(true, AtomicOrdering::SeqCst);
        let root = Value::from_serializable(data)?;
        let mut rendered = String::new();
        let mut scopes = vec![HashMap::new()];
        let flow = self.render_named(name, &root, &root, &mut scopes, &mut rendered, false)?;
        if !matches!(flow, RenderFlow::Normal) {
            return Err(TemplateError::Render(
                "break/continue action is not inside range".to_string(),
            ));
        }
        Ok(rendered)
    }

    pub fn lookup(&self, name: &str) -> Option<Self> {
        if self.templates.contains_key(name) {
            let mut clone = self.clone();
            clone.name = name.to_string();
            Some(clone)
        } else {
            None
        }
    }

    pub fn has_template(&self, name: &str) -> bool {
        self.templates.contains_key(name)
    }

    pub fn defined_templates(&self) -> Vec<String> {
        let mut names = self.templates.keys().cloned().collect::<Vec<_>>();
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

    pub fn templates(&self) -> Vec<Self> {
        let names = self.defined_templates();
        names
            .into_iter()
            .filter_map(|name| self.lookup(&name))
            .collect::<Vec<_>>()
    }

    fn parse_named(&mut self, name: &str, text: &str) -> Result<()> {
        let preprocessed = strip_html_comments(text);
        let tokens = tokenize(&preprocessed, &self.left_delim, &self.right_delim)?;
        let mut index = 0;
        let (nodes, stop) = parse_nodes(&tokens, &mut index, &[])?;
        if let Some(stop) = stop {
            return Err(TemplateError::Parse(format!(
                "unexpected control action `{}`",
                stop.keyword
            )));
        }

        let mut root_nodes = Vec::new();
        for node in nodes {
            match node {
                Node::Define {
                    name: defined_name,
                    body,
                } => {
                    self.templates.insert(defined_name, body);
                }
                Node::Block {
                    name: block_name,
                    data,
                    body,
                } => {
                    self.templates
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

        if !root_nodes.is_empty() || !self.templates.contains_key(name) {
            self.templates.insert(name.to_string(), root_nodes);
        }

        Ok(())
    }

    fn ensure_not_executed(&self) -> Result<()> {
        if self.executed.load(AtomicOrdering::SeqCst) {
            return Err(TemplateError::Parse(
                "template cannot be parsed or cloned after execution".to_string(),
            ));
        }
        Ok(())
    }

    fn reanalyze_contexts(&mut self) -> Result<()> {
        if !self.templates.contains_key(&self.name) {
            return Err(TemplateError::Parse(format!(
                "template `{}` is not defined",
                self.name
            )));
        }

        let mut analyzer = ParseContextAnalyzer::new(self.templates.clone());
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
        let mut names = self.templates.keys().cloned().collect::<Vec<_>>();
        names.sort();
        for name in names {
            if !analyzer.has_analysis(&name) {
                let _ = analyzer.analyze_template(&name, ContextState::html_text())?;
            }
        }

        self.templates = analyzer.finish();
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
    ) -> Result<RenderFlow> {
        let nodes = self
            .templates
            .get(name)
            .ok_or_else(|| TemplateError::Render(format!("template `{name}` is not defined")))?;
        self.render_nodes(nodes, root, dot, scopes, output, in_range)
    }

    fn render_nodes(
        &self,
        nodes: &[Node],
        root: &Value,
        dot: &Value,
        scopes: &mut ScopeStack,
        output: &mut String,
        in_range: bool,
    ) -> Result<RenderFlow> {
        for node in nodes {
            match node {
                Node::Text(text) => output.push_str(text),
                Node::Expr { expr, mode } => {
                    let value = self.eval_expr(expr, root, dot, scopes)?;
                    output.push_str(&escape_value_for_mode(&value, *mode)?);
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
                        let flow =
                            self.render_nodes(then_branch, root, dot, scopes, output, in_range)?;
                        pop_scope(scopes);
                        if !matches!(flow, RenderFlow::Normal) {
                            return Ok(flow);
                        }
                    } else {
                        push_scope(scopes);
                        let flow =
                            self.render_nodes(else_branch, root, dot, scopes, output, in_range)?;
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
                            self.render_nodes(else_branch, root, dot, scopes, output, true)?;
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
                                self.render_nodes(body, root, &item, scopes, output, true)?;
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
                            self.render_nodes(body, root, &value, scopes, output, in_range)?;
                        pop_scope(scopes);
                        if !matches!(flow, RenderFlow::Normal) {
                            return Ok(flow);
                        }
                    } else {
                        push_scope(scopes);
                        let flow =
                            self.render_nodes(else_branch, root, dot, scopes, output, in_range)?;
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
                    let mut template_scopes = vec![HashMap::new()];
                    let flow = self.render_named(
                        name,
                        root,
                        &next_dot,
                        &mut template_scopes,
                        output,
                        in_range,
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

                    if self.templates.contains_key(name) {
                        let mut template_scopes = vec![HashMap::new()];
                        let flow = self.render_named(
                            name,
                            root,
                            &next_dot,
                            &mut template_scopes,
                            output,
                            in_range,
                        )?;
                        if !matches!(flow, RenderFlow::Normal) {
                            return Ok(flow);
                        }
                    } else {
                        push_scope(scopes);
                        let flow =
                            self.render_nodes(body, root, &next_dot, scopes, output, in_range)?;
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

                    if name == "call" {
                        piped = Some(self.eval_call_function(&evaluated_args)?);
                    } else {
                        let function = self.funcs.get(name).ok_or_else(|| {
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
                lookup_path_with_methods(dot, path, &self.methods, self.missing_key_mode)
            }
            Term::RootPath(path) => {
                lookup_path_with_methods(root, path, &self.methods, self.missing_key_mode)
            }
            Term::Literal(value) => Ok(value.clone()),
            Term::Variable { name, path } => {
                let variable = lookup_variable(scopes, name).ok_or_else(|| {
                    TemplateError::Render(format!("variable `${name}` could not be resolved"))
                })?;
                lookup_path_with_methods(&variable, path, &self.methods, self.missing_key_mode)
            }
            Term::Identifier(name) => {
                if let Some(value) =
                    lookup_identifier(dot, root, name, &self.methods, self.missing_key_mode)?
                {
                    Ok(value)
                } else if self.funcs.contains_key(name) {
                    Ok(Value::FunctionRef(name.clone()))
                } else {
                    Err(TemplateError::Render(format!(
                        "identifier `{name}` could not be resolved"
                    )))
                }
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
                if let Some(method) = self.methods.get(name) {
                    return method(dot, args);
                }
                Err(TemplateError::Render(format!(
                    "callee `{name}` is not a callable method"
                )))
            }
            Term::Literal(_) => Err(TemplateError::Render(
                "literal values are not callable".to_string(),
            )),
        }
    }

    fn call_path_method(&self, base: &Value, path: &[String], args: &[Value]) -> Result<Value> {
        if path.is_empty() {
            return Err(TemplateError::Render("path is not callable".to_string()));
        }

        let (method_name, receiver_path) = split_last_path(path);
        let receiver =
            lookup_path_with_methods(base, receiver_path, &self.methods, self.missing_key_mode)?;
        let method = self.methods.get(method_name).ok_or_else(|| {
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

        let function = self
            .funcs
            .get(&name)
            .ok_or_else(|| TemplateError::Render(format!("function `{name}` is not registered")))?;
        function(&args[1..])
    }
}

pub fn must(result: Result<Template>) -> Template {
    match result {
        Ok(template) => template,
        Err(error) => panic!("{error}"),
    }
}

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

pub fn parse_glob(pattern: &str) -> Result<Template> {
    let paths = expand_glob_patterns([pattern])?;
    let name = template_name_from_path(&paths[0])?;
    Template::new(name).parse_files(paths)
}

pub fn parse_fs<I, S>(patterns: I) -> Result<Template>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let paths = expand_glob_patterns(patterns)?;
    let name = template_name_from_path(&paths[0])?;
    Template::new(name).parse_files(paths)
}

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
}

impl ContextState {
    fn html_text() -> Self {
        Self {
            mode: EscapeMode::Html,
            in_open_tag: false,
        }
    }

    fn from_rendered(rendered: &str) -> Self {
        let mode = infer_escape_mode(rendered);
        let in_open_tag = matches!(mode, EscapeMode::Html) && is_in_unclosed_tag_context(rendered);
        Self { mode, in_open_tag }
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
        self.normalize();
    }

    fn append_expr_placeholder(&mut self, mode: EscapeMode) {
        self.rendered.push_str(placeholder_for_mode(mode));
        self.normalize();
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
}

impl ParseContextAnalyzer {
    fn new(raw_templates: HashMap<String, Vec<Node>>) -> Self {
        Self {
            raw_templates,
            analyzed_templates: HashMap::new(),
            start_states: HashMap::new(),
            end_states: HashMap::new(),
            in_progress: HashSet::new(),
        }
    }

    fn has_analysis(&self, name: &str) -> bool {
        self.analyzed_templates.contains_key(name)
    }

    fn finish(self) -> HashMap<String, Vec<Node>> {
        self.analyzed_templates
    }

    fn analyze_template(&mut self, name: &str, start_state: ContextState) -> Result<ContextState> {
        if let Some(existing) = self.start_states.get(name) {
            if existing != &start_state {
                return Err(TemplateError::Parse(format!(
                    "cannot compute output context for template `{name}`"
                )));
            }

            if let Some(end) = self.end_states.get(name) {
                return Ok(end.clone());
            }

            if self.in_progress.contains(name) {
                return Err(TemplateError::Parse(format!(
                    "cannot compute output context for recursive template `{name}`"
                )));
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
            let start_tracker = ContextTracker::from_state(start_state);
            let flows = self.analyze_nodes(&mut nodes, start_tracker, false)?;

            let mut normal_states = HashSet::new();
            for flow in &flows {
                if flow.kind == AnalysisFlowKind::Normal {
                    normal_states.insert(flow.tracker.state());
                }
            }

            if normal_states.len() != 1 {
                return Err(TemplateError::Parse(format!(
                    "cannot compute output context for template `{name}`"
                )));
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
            Node::Expr { mode, .. } => {
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
                let range_start_state = range_start.state();
                let body_flows = self.analyze_nodes(body, range_start.clone(), true)?;
                let else_flows = self.analyze_nodes(else_branch, range_start.clone(), true)?;

                let mut output_flows = Vec::new();
                let mut natural_exit = true;

                for flow in body_flows {
                    match flow.kind {
                        AnalysisFlowKind::Normal | AnalysisFlowKind::Continue => {
                            if flow.tracker.state() != range_start_state {
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
                            return Err(TemplateError::Parse(
                                "on range loop re-entry: context mismatch".to_string(),
                            ));
                        }
                    }
                }

                ensure_single_normal_context("range", &output_flows)?;
                Ok(dedup_analysis_flows(output_flows))
            }
            Node::TemplateCall { name, .. } => {
                let start_state = tracker.state();
                let end_state = self.analyze_template(name, start_state)?;
                Ok(vec![AnalysisFlow::normal(ContextTracker::from_state(
                    end_state,
                ))])
            }
            Node::Block { name, body, .. } => {
                if self.raw_templates.contains_key(name) {
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
        if seen.insert((flow.kind, state.clone())) {
            deduped.push(AnalysisFlow::with_kind(
                flow.kind,
                ContextTracker::from_state(state),
            ));
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

    Ok(())
}

fn placeholder_for_mode(mode: EscapeMode) -> &'static str {
    match mode {
        EscapeMode::Html
        | EscapeMode::AttrQuoted { .. }
        | EscapeMode::AttrUnquoted { .. }
        | EscapeMode::ScriptString { .. }
        | EscapeMode::StyleExpr
        | EscapeMode::StyleString { .. } => "x",
        EscapeMode::ScriptExpr => "0",
    }
}

fn attr_name_for_kind(kind: AttrKind) -> &'static str {
    match kind {
        AttrKind::Normal => "title",
        AttrKind::Url => "href",
        AttrKind::Js => "onclick",
        AttrKind::Css => "style",
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
        EscapeMode::AttrQuoted { kind, quote } => {
            format!("<a {}={quote}x", attr_name_for_kind(kind))
        }
        EscapeMode::AttrUnquoted { kind } => format!("<a {}=x", attr_name_for_kind(kind)),
        EscapeMode::ScriptExpr => "<script>".to_string(),
        EscapeMode::ScriptString { quote } => format!("<script>{quote}"),
        EscapeMode::StyleExpr => "<style>".to_string(),
        EscapeMode::StyleString { quote } => format!("<style>{quote}"),
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

fn strip_html_comments(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0usize;

    while let Some(start_rel) = source[cursor..].find("<!--") {
        let start = cursor + start_rel;
        output.push_str(&source[cursor..start]);

        let comment_body_start = start + 4;
        if let Some(end_rel) = source[comment_body_start..].find("-->") {
            cursor = comment_body_start + end_rel + 3;
        } else {
            cursor = source.len();
            break;
        }
    }

    if cursor < source.len() {
        output.push_str(&source[cursor..]);
    }

    output
}

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
            action_start += 1;
            trim_last_text_whitespace(&mut tokens);
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

    Ok(Expr { commands })
}

fn split_pipeline(input: &str) -> Result<Vec<String>> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;

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
            '|' => {
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
            ch if ch.is_whitespace() => {
                if !current.is_empty() {
                    terms.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if quote.is_some() {
        return Err(TemplateError::Parse(
            "unterminated quoted string".to_string(),
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
    if let Some(path) = token.strip_prefix('.') {
        return Ok(Term::DotPath(parse_path(path)));
    }
    if let Some(reference) = token.strip_prefix('$') {
        let (name, path) = parse_variable_reference(reference)?;
        return Ok(Term::Variable { name, path });
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

    if let Ok(value) = token.parse::<i64>() {
        return Ok(Term::Literal(Value::from(value)));
    }
    if let Ok(value) = token.parse::<u64>() {
        return Ok(Term::Literal(Value::from(value)));
    }
    if let Ok(value) = token.parse::<f64>() {
        return Ok(Term::Literal(Value::from(value)));
    }

    if is_identifier(token) {
        return Ok(Term::Identifier(token.to_string()));
    }

    Err(TemplateError::Parse(format!(
        "unsupported token `{token}` in expression"
    )))
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
        Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn is_function_name(token: &str) -> bool {
    is_identifier(token) && token != "true" && token != "false" && token != "nil"
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
    Js,
    Css,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum EscapeMode {
    Html,
    AttrQuoted { kind: AttrKind, quote: char },
    AttrUnquoted { kind: AttrKind },
    ScriptExpr,
    ScriptString { quote: char },
    StyleExpr,
    StyleString { quote: char },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TagValueContext {
    attr_name: String,
    quoted: bool,
    quote: Option<char>,
}

fn escape_value_for_mode(value: &Value, mode: EscapeMode) -> Result<String> {
    match (value, mode) {
        (Value::SafeHtml(raw), EscapeMode::Html) => return Ok(raw.clone()),
        (Value::SafeHtmlAttr(raw), EscapeMode::AttrQuoted { .. })
        | (Value::SafeHtmlAttr(raw), EscapeMode::AttrUnquoted { .. }) => return Ok(raw.clone()),
        (Value::SafeJs(raw), EscapeMode::ScriptExpr)
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
        _ => {}
    }

    match mode {
        EscapeMode::Html => Ok(escape_html(&value.to_plain_string())),
        EscapeMode::AttrQuoted { kind, quote } => {
            let text = transform_attr_value(&value.to_plain_string(), kind, Some(quote));
            Ok(escape_html(&text))
        }
        EscapeMode::AttrUnquoted { kind } => {
            let text = transform_attr_value(&value.to_plain_string(), kind, None);
            Ok(escape_attr_unquoted(&text))
        }
        EscapeMode::ScriptExpr => escape_script_value(value),
        EscapeMode::ScriptString { quote } => {
            Ok(escape_js_string_fragment(&value.to_plain_string(), quote))
        }
        EscapeMode::StyleExpr => Ok(escape_css_text(&value.to_plain_string())),
        EscapeMode::StyleString { quote } => {
            Ok(escape_css_string_fragment(&value.to_plain_string(), quote))
        }
    }
}

fn infer_escape_mode(rendered: &str) -> EscapeMode {
    if let Some(context) = current_tag_value_context(rendered) {
        let kind = attr_kind(&context.attr_name);
        return if context.quoted {
            EscapeMode::AttrQuoted {
                kind,
                quote: context.quote.unwrap_or('"'),
            }
        } else {
            EscapeMode::AttrUnquoted { kind }
        };
    }

    if let Some(mode) = script_escape_mode(rendered) {
        return mode;
    }

    if let Some(mode) = style_escape_mode(rendered) {
        return mode;
    }

    EscapeMode::Html
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
                    });
                }
                return None;
            }
        }
    }

    None
}

fn is_url_attribute(attr_name: &str) -> bool {
    matches!(
        attr_name.to_ascii_lowercase().as_str(),
        "href" | "src" | "action" | "formaction" | "poster" | "data" | "srcset"
    )
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
    if xmlns_attr || is_url_attribute(&normalized) {
        AttrKind::Url
    } else if normalized.starts_with("on") {
        AttrKind::Js
    } else if normalized == "style" {
        AttrKind::Css
    } else {
        AttrKind::Normal
    }
}

fn transform_attr_value(value: &str, kind: AttrKind, quote: Option<char>) -> String {
    match kind {
        AttrKind::Normal => value.to_string(),
        AttrKind::Url => normalize_url_for_attribute(value),
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

fn script_escape_mode(rendered: &str) -> Option<EscapeMode> {
    let content = current_unclosed_tag_content(rendered, "script")?;
    if let Some(quote) = current_js_string_quote(content) {
        return Some(EscapeMode::ScriptString { quote });
    }
    Some(EscapeMode::ScriptExpr)
}

fn style_escape_mode(rendered: &str) -> Option<EscapeMode> {
    let content = current_unclosed_tag_content(rendered, "style")?;
    if let Some(quote) = current_css_string_quote(content) {
        return Some(EscapeMode::StyleString { quote });
    }
    Some(EscapeMode::StyleExpr)
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

fn current_js_string_quote(content: &str) -> Option<char> {
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut line_comment = false;
    let mut block_comment = false;

    while i < chars.len() {
        let ch = chars[i];
        let next = chars.get(i + 1).copied();

        if line_comment {
            if ch == '\n' {
                line_comment = false;
            }
            i += 1;
            continue;
        }

        if block_comment {
            if ch == '*' && next == Some('/') {
                block_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }

        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
                i += 1;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                i += 1;
                continue;
            }
            if ch == active_quote {
                quote = None;
            }
            i += 1;
            continue;
        }

        if ch == '/' && next == Some('/') {
            line_comment = true;
            i += 2;
            continue;
        }
        if ch == '/' && next == Some('*') {
            block_comment = true;
            i += 2;
            continue;
        }

        if ch == '"' || ch == '\'' || ch == '`' {
            quote = Some(ch);
        }
        i += 1;
    }

    quote
}

fn current_css_string_quote(content: &str) -> Option<char> {
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut block_comment = false;

    while i < chars.len() {
        let ch = chars[i];
        let next = chars.get(i + 1).copied();

        if block_comment {
            if ch == '*' && next == Some('/') {
                block_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }

        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
                i += 1;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                i += 1;
                continue;
            }
            if ch == active_quote {
                quote = None;
            }
            i += 1;
            continue;
        }

        if ch == '/' && next == Some('*') {
            block_comment = true;
            i += 2;
            continue;
        }

        if ch == '"' || ch == '\'' {
            quote = Some(ch);
        }
        i += 1;
    }

    quote
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
    let mut chars = trimmed.chars().peekable();
    let mut scheme = String::new();

    while let Some(ch) = chars.peek().copied() {
        if ch == ':' {
            if scheme.is_empty() {
                return true;
            }
            let scheme = scheme.to_ascii_lowercase();
            return !matches!(scheme.as_str(), "javascript" | "vbscript" | "data");
        }
        if ch == '/' || ch == '?' || ch == '#' {
            return true;
        }
        if scheme.is_empty() {
            if ch.is_ascii_alphabetic() {
                scheme.push(ch);
                chars.next();
                continue;
            }
            return true;
        }
        if ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.' {
            scheme.push(ch);
            chars.next();
            continue;
        }
        return true;
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
            ' ' => escaped.push_str("&#32;"),
            '\n' => escaped.push_str("&#10;"),
            '\r' => escaped.push_str("&#13;"),
            '\t' => escaped.push_str("&#9;"),
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
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

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
            "<div data-v=a&#32;b&#61;&lt;x&gt; title=\"&quot;q&quot; &amp; &lt;t&gt;\"></div>"
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
    fn slice_function_supports_string_and_array() {
        let template = Template::new("slice")
            .parse("{{slice .S 1 4}}|{{slice .A 1 3}}")
            .expect("parse should succeed");

        let output = template
            .execute_to_string(&json!({"S": "abcdef", "A": ["x", "y", "z", "w"]}))
            .expect("execute should succeed");

        assert_eq!(output, "bcd|[&quot;y&quot;,&quot;z&quot;]");
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
    }

    #[test]
    fn parse_time_rejects_missing_template_reference() {
        let error = match Template::new("main").parse("<div>{{template \"missing\" .}}</div>") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("no such template"));
    }

    #[test]
    fn parse_time_rejects_non_text_end_context() {
        let error = match Template::new("end").parse("<div title=\"{{.X}}") {
            Ok(_) => panic!("parse should fail"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("non-text context"));
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
