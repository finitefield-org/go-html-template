use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

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
}

impl Value {
    pub fn safe_html<S: Into<String>>(value: S) -> Self {
        Self::SafeHtml(value.into())
    }

    fn from_serializable<T: Serialize>(data: &T) -> Result<Self> {
        Ok(Self::Json(serde_json::to_value(data)?))
    }

    fn truthy(&self) -> bool {
        match self {
            Value::SafeHtml(value) => !value.is_empty(),
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
            Value::Json(value) => match value {
                JsonValue::Null => String::new(),
                JsonValue::Bool(v) => v.to_string(),
                JsonValue::Number(v) => v.to_string(),
                JsonValue::String(v) => v.clone(),
                JsonValue::Array(_) | JsonValue::Object(_) => value.to_string(),
            },
        }
    }

    fn html_output(&self) -> String {
        match self {
            Value::SafeHtml(value) => value.clone(),
            Value::Json(_) => escape_html(&self.to_plain_string()),
        }
    }

    fn iter_pairs(&self) -> Vec<(Value, Value)> {
        match self {
            Value::Json(JsonValue::Array(items)) => items
                .iter()
                .enumerate()
                .map(|(index, value)| (Value::from(index as u64), Value::Json(value.clone())))
                .collect::<Vec<_>>(),
            Value::Json(JsonValue::Object(items)) => items
                .iter()
                .map(|(key, value)| (Value::from(key.as_str()), Value::Json(value.clone())))
                .collect::<Vec<_>>(),
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
            _ => Vec::new(),
        }
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
type ScopeStack = Vec<HashMap<String, Value>>;

#[derive(Clone)]
pub struct Template {
    name: String,
    templates: HashMap<String, Vec<Node>>,
    funcs: FuncMap,
}

impl Template {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            templates: HashMap::new(),
            funcs: builtin_funcs(),
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

    pub fn parse(mut self, text: &str) -> Result<Self> {
        let root = self.name.clone();
        self.parse_named(&root, text)?;
        Ok(self)
    }

    pub fn parse_files<I, P>(mut self, paths: I) -> Result<Self>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
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

        Ok(self)
    }

    pub fn parse_glob(self, pattern: &str) -> Result<Self> {
        let mut paths = Vec::new();
        for entry in glob::glob(pattern)? {
            paths.push(entry?);
        }
        paths.sort();
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
        let root = Value::from_serializable(data)?;
        let mut rendered = String::new();
        let mut scopes = vec![HashMap::new()];
        self.render_named(name, &root, &root, &mut scopes, &mut rendered)?;
        writer.write_all(rendered.as_bytes())?;
        Ok(())
    }

    pub fn execute_template_to_string<T: Serialize>(&self, name: &str, data: &T) -> Result<String> {
        let root = Value::from_serializable(data)?;
        let mut rendered = String::new();
        let mut scopes = vec![HashMap::new()];
        self.render_named(name, &root, &root, &mut scopes, &mut rendered)?;
        Ok(rendered)
    }

    pub fn lookup(&self, name: &str) -> bool {
        self.templates.contains_key(name)
    }

    pub fn defined_templates(&self) -> Vec<String> {
        let mut names = self.templates.keys().cloned().collect::<Vec<_>>();
        names.sort();
        names
    }

    fn parse_named(&mut self, name: &str, text: &str) -> Result<()> {
        let tokens = tokenize(text)?;
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

    fn render_named(
        &self,
        name: &str,
        root: &Value,
        dot: &Value,
        scopes: &mut ScopeStack,
        output: &mut String,
    ) -> Result<()> {
        let nodes = self
            .templates
            .get(name)
            .ok_or_else(|| TemplateError::Render(format!("template `{name}` is not defined")))?;
        self.render_nodes(nodes, root, dot, scopes, output)
    }

    fn render_nodes(
        &self,
        nodes: &[Node],
        root: &Value,
        dot: &Value,
        scopes: &mut ScopeStack,
        output: &mut String,
    ) -> Result<()> {
        for node in nodes {
            match node {
                Node::Text(text) => output.push_str(text),
                Node::Expr(expr) => {
                    let value = self.eval_expr(expr, root, dot, scopes)?;
                    output.push_str(&value.html_output());
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
                        self.render_nodes(then_branch, root, dot, scopes, output)?;
                        pop_scope(scopes);
                    } else {
                        push_scope(scopes);
                        self.render_nodes(else_branch, root, dot, scopes, output)?;
                        pop_scope(scopes);
                    }
                }
                Node::Range {
                    vars,
                    iterable,
                    body,
                    else_branch,
                } => {
                    let iterable_value = self.eval_expr(iterable, root, dot, scopes)?;
                    let items = iterable_value.iter_pairs();
                    if items.is_empty() {
                        push_scope(scopes);
                        self.render_nodes(else_branch, root, dot, scopes, output)?;
                        pop_scope(scopes);
                    } else {
                        for (key, item) in items {
                            push_scope(scopes);
                            if vars.len() == 1 {
                                declare_variable(scopes, &vars[0], item.clone());
                            } else if vars.len() == 2 {
                                declare_variable(scopes, &vars[0], key);
                                declare_variable(scopes, &vars[1], item.clone());
                            }
                            self.render_nodes(body, root, &item, scopes, output)?;
                            pop_scope(scopes);
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
                        self.render_nodes(body, root, &value, scopes, output)?;
                        pop_scope(scopes);
                    } else {
                        push_scope(scopes);
                        self.render_nodes(else_branch, root, dot, scopes, output)?;
                        pop_scope(scopes);
                    }
                }
                Node::TemplateCall { name, data } => {
                    let next_dot = match data {
                        Some(expr) => self.eval_expr(expr, root, dot, scopes)?,
                        None => dot.clone(),
                    };
                    let mut template_scopes = vec![HashMap::new()];
                    self.render_named(name, root, &next_dot, &mut template_scopes, output)?;
                }
                Node::Block { name, data, body } => {
                    let next_dot = match data {
                        Some(expr) => self.eval_expr(expr, root, dot, scopes)?,
                        None => dot.clone(),
                    };

                    if self.templates.contains_key(name) {
                        let mut template_scopes = vec![HashMap::new()];
                        self.render_named(name, root, &next_dot, &mut template_scopes, output)?;
                    } else {
                        push_scope(scopes);
                        self.render_nodes(body, root, &next_dot, scopes, output)?;
                        pop_scope(scopes);
                    }
                }
                Node::Define { .. } => {}
            }
        }

        Ok(())
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

                    let function = self.funcs.get(name).ok_or_else(|| {
                        TemplateError::Render(format!("function `{name}` is not registered"))
                    })?;
                    piped = Some(function(&evaluated_args)?);
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
            Term::DotPath(path) => Ok(lookup_path(dot, path)),
            Term::RootPath(path) => Ok(lookup_path(root, path)),
            Term::Literal(value) => Ok(value.clone()),
            Term::Variable { name, path } => {
                let variable = lookup_variable(scopes, name).ok_or_else(|| {
                    TemplateError::Render(format!("variable `${name}` could not be resolved"))
                })?;
                Ok(lookup_path(&variable, path))
            }
            Term::Identifier(name) => lookup_identifier(dot, root, name).ok_or_else(|| {
                TemplateError::Render(format!("identifier `{name}` could not be resolved"))
            }),
        }
    }
}

#[derive(Clone, Debug)]
enum Node {
    Text(String),
    Expr(Expr),
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
}

#[derive(Clone, Debug)]
struct Expr {
    commands: Vec<Command>,
}

#[derive(Clone, Debug)]
enum Command {
    Value(Term),
    Call { name: String, args: Vec<Term> },
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
                .all(|candidate| values_equal(first, candidate));
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

fn lookup_path(base: &Value, path: &[String]) -> Value {
    if path.is_empty() {
        return base.clone();
    }

    let mut current = match base {
        Value::Json(value) => value,
        Value::SafeHtml(_) => return Value::Json(JsonValue::Null),
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

fn lookup_identifier(dot: &Value, root: &Value, name: &str) -> Option<Value> {
    lookup_object_key(dot, name).or_else(|| lookup_object_key(root, name))
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

fn tokenize(source: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut cursor = 0usize;

    while let Some(start_offset) = source[cursor..].find("{{") {
        let start = cursor + start_offset;
        if start > cursor {
            tokens.push(Token::Text(source[cursor..start].to_string()));
        }

        let mut action_start = start + 2;
        if source[action_start..].starts_with('-') {
            action_start += 1;
            trim_last_text_whitespace(&mut tokens);
        }

        let end_offset = source[action_start..]
            .find("}}")
            .ok_or_else(|| TemplateError::Parse("unclosed action (missing `}}`)".to_string()))?;
        let end = action_start + end_offset;

        let mut action = source[action_start..end].trim().to_string();
        let trim_right = action.ends_with('-');
        if trim_right {
            action.pop();
            action = action.trim_end().to_string();
        }

        tokens.push(Token::Action(action));
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
                        let (vars, iterable) = parse_range_clause(tail)?;
                        let (body, else_branch) =
                            parse_optional_else_block(tokens, index, "range")?;
                        nodes.push(Node::Range {
                            vars,
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
                        let (body, else_branch) = parse_optional_else_block(tokens, index, "with")?;
                        nodes.push(Node::With {
                            value,
                            body,
                            else_branch,
                        });
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
                    "else" | "end" => {
                        return Err(TemplateError::Parse(format!("unexpected `{head}`")));
                    }
                    _ => {
                        if let Some(set_var) = parse_variable_assignment_action(action)? {
                            nodes.push(set_var);
                        } else {
                            let expr = parse_expression(action)?;
                            nodes.push(Node::Expr(expr));
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

fn parse_range_clause(input: &str) -> Result<(Vec<String>, Expr)> {
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
        return Ok((vars, iterable));
    }

    Ok((Vec::new(), parse_expression(input)?))
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
            if terms.len() != 1 {
                return Err(TemplateError::Parse(format!(
                    "invalid expression segment `{segment}`"
                )));
            }
            commands.push(Command::Value(parse_term(&terms[0])?));
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
}
