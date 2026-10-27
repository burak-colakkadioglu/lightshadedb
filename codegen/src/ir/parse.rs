use super::ast::*;
use std::collections::HashSet;
use std::fmt;

#[derive(Debug)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl ParseError {
    fn new(line: usize, message: impl Into<String>) -> Self {
        ParseError {
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

#[derive(Clone)]
struct Line {
    no: usize,
    text: String,
}

struct Reader {
    lines: Vec<Line>,
    pos: usize,
}

impl Reader {
    fn next(&mut self) -> Option<Line> {
        let line = self.lines.get(self.pos).cloned();
        if line.is_some() {
            self.pos += 1;
        }
        line
    }
}

pub fn parse_workload(source: &str) -> Result<Workload, ParseError> {
    let lines = clean_lines(source);
    let mut reader = Reader { lines, pos: 0 };
    let mut workload = Workload::default();

    while let Some(line) = reader.next() {
        if line.text.starts_with("table ") {
            parse_table(&mut reader, line, &mut workload)?;
        } else if line.text.starts_with("operation ") {
            parse_operation(&mut reader, line, &mut workload)?;
        } else {
            return Err(ParseError::new(
                line.no,
                "expected `table <name> {` or `operation <name>(...) {`",
            ));
        }
    }

    Ok(workload)
}

fn clean_lines(source: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    for (index, raw) in source.lines().enumerate() {
        let text = strip_comment(raw);
        let text = text.trim();
        if !text.is_empty() {
            lines.push(Line {
                no: index + 1,
                text: text.to_string(),
            });
        }
    }
    lines
}

fn strip_comment(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars();
    let mut in_string = false;

    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if ch == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if ch == '"' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
            out.push(ch);
        } else if ch == '#' {
            break;
        } else {
            out.push(ch);
        }
    }

    out
}

fn parse_table(
    reader: &mut Reader,
    header: Line,
    workload: &mut Workload,
) -> Result<(), ParseError> {
    let rest = header.text.strip_prefix("table ").unwrap().trim();
    let (name, after) = rest
        .split_once('{')
        .ok_or_else(|| ParseError::new(header.no, "expected `table <name> {`"))?;
    if !after.trim().is_empty() {
        return Err(ParseError::new(header.no, "unexpected text after `{`"));
    }
    let parts: Vec<&str> = name.split_whitespace().collect();
    let (table_name, ttl_secs) = match parts.as_slice() {
        [table] => (*table, None),
        [table, ttl_keyword, secs] if *ttl_keyword == "ttl" => {
            let parsed: u64 = secs.parse().map_err(|_| {
                ParseError::new(
                    header.no,
                    "ttl must be a positive integer number of seconds",
                )
            })?;
            if parsed == 0 {
                return Err(ParseError::new(header.no, "ttl must be greater than zero"));
            }
            (*table, Some(parsed))
        }
        _ => {
            return Err(ParseError::new(
                header.no,
                "expected `table <name> [ttl <secs>] {`",
            ));
        }
    };
    let name = parse_ident(table_name).map_err(|message| ParseError::new(header.no, message))?;

    let mut fields = Vec::new();
    loop {
        let line = reader.next().ok_or_else(|| {
            ParseError::new(
                header.no,
                format!("table `{name}` is missing its closing `}}`"),
            )
        })?;
        if line.text == "}" {
            break;
        }
        fields.push(parse_field(line)?);
    }

    workload.tables.push(Table {
        name,
        fields,
        ttl_secs,
    });
    Ok(())
}

fn parse_field(line: Line) -> Result<Field, ParseError> {
    let (name, rest) = line
        .text
        .split_once(':')
        .ok_or_else(|| ParseError::new(line.no, "expected `name: type` field"))?;
    let name = parse_ident(name.trim()).map_err(|message| ParseError::new(line.no, message))?;
    let rest = rest.trim();
    let mut default_text = None;
    let head = match rest.split_once("default") {
        Some((before, after)) if before.ends_with(' ') || before.is_empty() => {
            default_text = Some(after.trim());
            before.trim_end()
        }
        _ => rest,
    };
    let mut parts = head.split_whitespace();
    let type_text = parts
        .next()
        .ok_or_else(|| ParseError::new(line.no, "expected a field type"))?;
    let ty = parse_type(type_text).map_err(|message| ParseError::new(line.no, message))?;

    let mut primary_key = false;
    for flag in parts {
        match flag {
            "primary_key" if !primary_key => primary_key = true,
            "primary_key" => {
                return Err(ParseError::new(line.no, "duplicate `primary_key` modifier"))
            }
            other => {
                return Err(ParseError::new(
                    line.no,
                    format!("unknown field modifier `{other}`"),
                ))
            }
        }
    }

    let default = match default_text {
        None => None,
        Some(text) => {
            if text.is_empty() {
                return Err(ParseError::new(line.no, "expected a value after `default`"));
            }
            let tokens = tokenize(text).map_err(|message| ParseError::new(line.no, message))?;
            let value = match tokens.as_slice() {
                [Tok::Num(value)] => {
                    integer_literal(*value).map_err(|message| ParseError::new(line.no, message))?
                }
                [Tok::Float(value)] => Value::F64(*value),
                [Tok::Str(value)] => Value::Text(value.clone()),
                [Tok::True] => Value::Bool(true),
                [Tok::False] => Value::Bool(false),
                _ => {
                    return Err(ParseError::new(
                        line.no,
                        "expected a single literal after `default`",
                    ))
                }
            };
            let Some(value) = coerce_literal(value, ty) else {
                return Err(ParseError::new(
                    line.no,
                    format!("default value does not match field type `{type_text}`"),
                ));
            };
            Some(value)
        }
    };

    Ok(Field {
        name,
        ty,
        primary_key,
        default,
        nullable: false,
    })
}

fn parse_operation(
    reader: &mut Reader,
    header: Line,
    workload: &mut Workload,
) -> Result<(), ParseError> {
    let (name, params, returns) = parse_operation_header(&header.text)
        .map_err(|message| ParseError::new(header.no, message))?;

    let param_names: HashSet<String> = params.iter().map(|param| param.name.clone()).collect();

    let mut filter: Option<Expr> = None;
    let mut order: Vec<(String, bool)> = Vec::new();
    let mut limit: Option<Limit> = None;
    let mut offset: Option<Limit> = None;
    let mut source: Option<(String, Option<String>)> = None;
    let mut joins: Vec<Join> = Vec::new();
    let mut select: Vec<SelectItem> = Vec::new();
    let mut group_by: Vec<FieldRef> = Vec::new();
    let mut terminal: Option<Body> = None;

    loop {
        let line = reader.next().ok_or_else(|| {
            ParseError::new(
                header.no,
                format!("operation `{name}` is missing its closing `}}`"),
            )
        })?;
        if line.text == "}" {
            break;
        }

        if let Some(rest) = line.text.strip_prefix("filter ") {
            require_no_terminal(&terminal, &line)?;
            if filter.is_some() {
                return Err(ParseError::new(line.no, "duplicate `filter` clause"));
            }
            filter = Some(parse_expr(rest, &param_names, line.no)?);
        } else if let Some(rest) = line.text.strip_prefix("order by ") {
            require_no_terminal(&terminal, &line)?;
            if !order.is_empty() {
                return Err(ParseError::new(line.no, "duplicate `order by` clause"));
            }
            order = parse_order(rest, line.no)?;
        } else if let Some(rest) = line.text.strip_prefix("limit ") {
            require_no_terminal(&terminal, &line)?;
            if limit.is_some() {
                return Err(ParseError::new(line.no, "duplicate `limit` clause"));
            }
            let rest = rest.trim();
            if rest.is_empty() {
                return Err(ParseError::new(line.no, "expected `limit` value"));
            }
            if let Ok(value) = rest.parse::<u64>() {
                limit = Some(Limit::Literal(value));
            } else {
                limit = Some(Limit::Param(
                    parse_ident(rest).map_err(|message| ParseError::new(line.no, message))?,
                ));
            }
        } else if let Some(rest) = line.text.strip_prefix("offset ") {
            require_no_terminal(&terminal, &line)?;
            if offset.is_some() {
                return Err(ParseError::new(line.no, "duplicate `offset` clause"));
            }
            let rest = rest.trim();
            if rest.is_empty() {
                return Err(ParseError::new(line.no, "expected `offset` value"));
            }
            if let Ok(value) = rest.parse::<u64>() {
                offset = Some(Limit::Literal(value));
            } else {
                offset = Some(Limit::Param(
                    parse_ident(rest).map_err(|message| ParseError::new(line.no, message))?,
                ));
            }
        } else if let Some(rest) = line.text.strip_prefix("from ") {
            require_no_terminal(&terminal, &line)?;
            if source.is_some() {
                return Err(ParseError::new(line.no, "duplicate `from` clause"));
            }
            source = Some(parse_source(rest, line.no)?);
        } else if let Some(rest) = line.text.strip_prefix("join ") {
            require_no_terminal(&terminal, &line)?;
            joins.push(parse_join(rest, line.no)?);
        } else if let Some(rest) = line.text.strip_prefix("select ") {
            require_no_terminal(&terminal, &line)?;
            if !select.is_empty() {
                return Err(ParseError::new(line.no, "duplicate `select` clause"));
            }
            select = parse_select(rest, line.no)?;
        } else if let Some(rest) = line.text.strip_prefix("group by ") {
            require_no_terminal(&terminal, &line)?;
            if !group_by.is_empty() {
                return Err(ParseError::new(line.no, "duplicate `group by` clause"));
            }
            group_by = parse_group_by(rest, line.no)?;
        } else if let Some(rest) = line.text.strip_prefix("count ") {
            require_no_terminal(&terminal, &line)?;
            require_no_query_clauses(&filter, &order, &limit, &offset, &line)?;
            require_no_advanced(&source, &joins, &select, &group_by, &line)?;
            terminal = Some(parse_count(rest, &param_names, line.no)?);
        } else if let Some(rest) = line.text.strip_prefix("update ") {
            require_no_terminal(&terminal, &line)?;
            require_no_query_clauses(&filter, &order, &limit, &offset, &line)?;
            require_no_advanced(&source, &joins, &select, &group_by, &line)?;
            terminal = Some(parse_update(rest, &param_names, line.no)?);
        } else if let Some(rest) = line.text.strip_prefix("delete ") {
            require_no_terminal(&terminal, &line)?;
            require_no_query_clauses(&filter, &order, &limit, &offset, &line)?;
            require_no_advanced(&source, &joins, &select, &group_by, &line)?;
            terminal = Some(parse_delete(rest, &param_names, line.no)?);
        } else if let Some(rest) = line.text.strip_prefix("insert ") {
            require_no_terminal(&terminal, &line)?;
            require_no_query_clauses(&filter, &order, &limit, &offset, &line)?;
            require_no_advanced(&source, &joins, &select, &group_by, &line)?;
            let rest = rest.trim();
            if rest.is_empty() {
                return Err(ParseError::new(
                    line.no,
                    "expected a table name after `insert`",
                ));
            }
            if rest.split_whitespace().count() != 1 {
                return Err(ParseError::new(
                    line.no,
                    "unexpected text after `insert <table>`",
                ));
            }
            terminal = Some(Body::Insert {
                table: parse_ident(rest).map_err(|message| ParseError::new(line.no, message))?,
            });
        } else {
            return Err(ParseError::new(
                line.no,
                format!("unknown operation statement `{}`", line.text),
            ));
        }
    }

    let body = if let Some(body) = terminal {
        body
    } else if let Returns::Many(table) = &returns {
        let (table, alias) = match source {
            Some(source) => source,
            None => (table.clone(), None),
        };
        Body::Query {
            table,
            alias,
            joins,
            select,
            group_by,
            filter,
            order,
            limit,
            offset,
            count: false,
        }
    } else {
        return Err(ParseError::new(
            header.no,
            "operation needs `count`, `update`, `delete`, `insert`, or a `-> many <table>` query body",
        ));
    };

    workload.operations.push(Operation {
        name,
        params,
        returns,
        body,
    });
    Ok(())
}

fn parse_operation_header(text: &str) -> Result<(String, Vec<Param>, Returns), String> {
    let rest = text
        .strip_prefix("operation ")
        .ok_or("expected `operation`")?
        .trim();
    let (name, rest) = rest
        .split_once('(')
        .ok_or("expected `(` after the operation name")?;
    let name = parse_ident(name.trim())?;
    let (params_text, rest) = rest
        .split_once(')')
        .ok_or("expected `)` after the operation parameters")?;

    let rest = rest.trim();
    let (return_text, after) = if let Some(return_part) = rest.strip_prefix("->") {
        let (return_text, after) = return_part
            .split_once('{')
            .ok_or("expected `{` after the return type")?;
        (return_text.trim(), after)
    } else {
        let (before, after) = rest
            .split_once('{')
            .ok_or("expected `{` after the operation signature")?;
        if !before.trim().is_empty() {
            return Err("expected `{` after the operation signature".to_string());
        }
        ("", after)
    };
    if !after.trim().is_empty() {
        return Err("unexpected text after `{`".to_string());
    }

    let returns = if return_text.is_empty() {
        Returns::None
    } else {
        let mut parts = return_text.split_whitespace();
        let kind = parts.next().ok_or("expected a return type")?;
        let argument = parts.next().ok_or("return type is incomplete")?;
        if parts.next().is_some() {
            return Err(format!("unexpected text in return type `{return_text}`"));
        }
        match kind {
            "one" => Returns::One(parse_type(argument)?),
            "many" => Returns::Many(parse_ident(argument)?),
            other => {
                return Err(format!(
                    "expected `one <type>` or `many <table>`, found `{other}`"
                ))
            }
        }
    };

    Ok((name, parse_params(params_text)?, returns))
}

fn parse_params(text: &str) -> Result<Vec<Param>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Vec::new());
    }

    let mut params = Vec::new();
    for part in text.split(',') {
        let (name, ty) = part
            .split_once(':')
            .ok_or("expected a `name: type` parameter")?;
        params.push(Param {
            name: parse_ident(name.trim())?,
            ty: parse_type(ty.trim())?,
            nullable: false,
        });
    }
    Ok(params)
}

fn parse_order(text: &str, line: usize) -> Result<Vec<(String, bool)>, ParseError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(ParseError::new(line, "expected an `order by` field"));
    }

    let mut order = Vec::new();
    for part in text.split(',') {
        let mut parts = part.split_whitespace();
        let field = parts
            .next()
            .ok_or_else(|| ParseError::new(line, "expected an `order by` field"))?;
        let field = parse_ident(field).map_err(|message| ParseError::new(line, message))?;
        let descending = match parts.next() {
            None | Some("asc") => false,
            Some("desc") => true,
            Some(other) => {
                return Err(ParseError::new(
                    line,
                    format!("expected `asc` or `desc`, found `{other}`"),
                ))
            }
        };
        if parts.next().is_some() {
            return Err(ParseError::new(line, "unexpected text in `order by`"));
        }
        order.push((field, descending));
    }
    Ok(order)
}

fn parse_count(text: &str, params: &HashSet<String>, line: usize) -> Result<Body, ParseError> {
    let mut parts = text.splitn(2, char::is_whitespace);
    let table = parts.next().unwrap_or("").trim();
    if table.is_empty() {
        return Err(ParseError::new(
            line,
            "expected `count <table> where <expr>`",
        ));
    }
    let rest = parts.next().unwrap_or("").trim_start();
    let filter_text = rest
        .strip_prefix("where ")
        .ok_or_else(|| ParseError::new(line, "expected `where` after the count table"))?;
    Ok(Body::Query {
        table: parse_ident(table).map_err(|message| ParseError::new(line, message))?,
        alias: None,
        joins: Vec::new(),
        select: Vec::new(),
        group_by: Vec::new(),
        filter: Some(parse_expr(filter_text, params, line)?),
        order: Vec::new(),
        limit: None,
        offset: None,
        count: true,
    })
}

fn require_no_advanced(
    source: &Option<(String, Option<String>)>,
    joins: &[Join],
    select: &[SelectItem],
    group_by: &[FieldRef],
    line: &Line,
) -> Result<(), ParseError> {
    if source.is_some() || !joins.is_empty() || !select.is_empty() || !group_by.is_empty() {
        return Err(ParseError::new(
            line.no,
            "`count`, `update`, `delete`, and `insert` cannot be combined with query clauses",
        ));
    }
    Ok(())
}

fn parse_source(text: &str, line: usize) -> Result<(String, Option<String>), ParseError> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    match parts.as_slice() {
        [table] => Ok((
            parse_ident(table).map_err(|message| ParseError::new(line, message))?,
            None,
        )),
        [table, "as", alias] => Ok((
            parse_ident(table).map_err(|message| ParseError::new(line, message))?,
            Some(parse_ident(alias).map_err(|message| ParseError::new(line, message))?),
        )),
        _ => Err(ParseError::new(
            line,
            "expected `<table>` or `<table> as <alias>`",
        )),
    }
}

fn parse_field_ref(text: &str, line: usize) -> Result<FieldRef, ParseError> {
    let text = text.trim();
    let (qualifier, field) = match text.split_once('.') {
        Some((qualifier, field)) => {
            if field.contains('.') {
                return Err(ParseError::new(line, "expected `alias.field`"));
            }
            (Some(qualifier), field)
        }
        None => (None, text),
    };
    let field = parse_ident(field).map_err(|message| ParseError::new(line, message))?;
    let qualifier = match qualifier {
        Some(qualifier) => {
            Some(parse_ident(qualifier).map_err(|message| ParseError::new(line, message))?)
        }
        None => None,
    };
    Ok(FieldRef { qualifier, field })
}

fn parse_join(text: &str, line: usize) -> Result<Join, ParseError> {
    let (source, condition) = text.split_once(" on ").ok_or_else(|| {
        ParseError::new(
            line,
            "expected `join <table> [as <alias>] on <left> == <right>`",
        )
    })?;
    let (table, alias) = parse_source(source, line)?;
    let (left, right) = condition
        .split_once(" == ")
        .ok_or_else(|| ParseError::new(line, "join conditions must be `<field> == <field>`"))?;
    if right.contains("==") {
        return Err(ParseError::new(
            line,
            "join conditions must be a single equality",
        ));
    }
    Ok(Join {
        table,
        alias,
        keys: vec![JoinKey {
            left: parse_field_ref(left, line)?,
            right: parse_field_ref(right, line)?,
        }],
    })
}

fn parse_select(text: &str, line: usize) -> Result<Vec<SelectItem>, ParseError> {
    let mut items = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(ParseError::new(line, "expected a select item"));
        }
        let (expr_text, alias) = match part.rsplit_once(" as ") {
            Some((expr, alias)) => (
                expr.trim(),
                Some(parse_ident(alias.trim()).map_err(|message| ParseError::new(line, message))?),
            ),
            None => (part, None),
        };
        let mut aggregate = None;
        for function in [
            Aggregate::Count,
            Aggregate::Sum,
            Aggregate::Min,
            Aggregate::Max,
            Aggregate::Avg,
        ] {
            let prefix = format!("{}(", function.name());
            if let Some(rest) = expr_text.strip_prefix(&prefix) {
                let inner = rest
                    .strip_suffix(')')
                    .ok_or_else(|| ParseError::new(line, "aggregate call is missing `)`"))?;
                aggregate = Some((function, inner.trim()));
                break;
            }
        }
        let (expr, default_alias) = match aggregate {
            Some((function, inner)) => {
                let field = if function == Aggregate::Count {
                    if inner.is_empty() || inner == "*" {
                        None
                    } else {
                        return Err(ParseError::new(
                            line,
                            "`count` does not take a field; use `count()`",
                        ));
                    }
                } else {
                    if inner.is_empty() {
                        return Err(ParseError::new(
                            line,
                            format!("`{}` needs a field", function.name()),
                        ));
                    }
                    Some(parse_field_ref(inner, line)?)
                };
                (
                    SelectExpr::Aggregate { function, field },
                    function.name().to_string(),
                )
            }
            None => {
                let field = parse_field_ref(expr_text, line)?;
                let default_alias = field.field.clone();
                (SelectExpr::Field(field), default_alias)
            }
        };
        let alias = match alias {
            Some(alias) => alias,
            None => {
                if matches!(expr, SelectExpr::Aggregate { .. }) {
                    return Err(ParseError::new(
                        line,
                        "aggregate select items need `as <name>`",
                    ));
                }
                default_alias
            }
        };
        items.push(SelectItem { expr, alias });
    }
    Ok(items)
}

fn parse_group_by(text: &str, line: usize) -> Result<Vec<FieldRef>, ParseError> {
    let mut refs = Vec::new();
    for part in text.split(',') {
        refs.push(parse_field_ref(part, line)?);
    }
    Ok(refs)
}

fn parse_update(text: &str, params: &HashSet<String>, line: usize) -> Result<Body, ParseError> {
    let mut parts = text.splitn(2, char::is_whitespace);
    let table = parts.next().unwrap_or("").trim();
    if table.is_empty() {
        return Err(ParseError::new(
            line,
            "expected `update <table> set <field> = <value> [where <expr>]`",
        ));
    }
    let rest = parts.next().unwrap_or("").trim_start();
    let rest = rest
        .strip_prefix("set")
        .map(str::trim_start)
        .ok_or_else(|| ParseError::new(line, "expected `set` after the update table"))?;

    let (sets_text, filter_text) = match split_keyword(rest, "where") {
        Some((sets, filter)) => (sets.trim(), Some(filter.trim())),
        None => (rest.trim(), None),
    };
    if sets_text.is_empty() {
        return Err(ParseError::new(
            line,
            "expected at least one `set` assignment",
        ));
    }

    let mut sets = Vec::new();
    for part in split_outside_strings(sets_text, ',') {
        let (field, value) = part
            .split_once('=')
            .ok_or_else(|| ParseError::new(line, "expected `field = value` in `set`"))?;
        if value.trim_start().starts_with('=') {
            return Err(ParseError::new(
                line,
                "use a single `=` in `set` assignments",
            ));
        }
        let field = parse_ident(field.trim()).map_err(|message| ParseError::new(line, message))?;
        let value = parse_operand_text(value.trim(), params, line)?;
        sets.push((field, value));
    }

    let filter = match filter_text {
        Some(text) if !text.is_empty() => Some(parse_expr(text, params, line)?),
        Some(_) => {
            return Err(ParseError::new(
                line,
                "expected an expression after `where`",
            ))
        }
        None => None,
    };

    Ok(Body::Update {
        table: parse_ident(table).map_err(|message| ParseError::new(line, message))?,
        sets,
        filter,
    })
}

fn parse_delete(text: &str, params: &HashSet<String>, line: usize) -> Result<Body, ParseError> {
    let mut parts = text.splitn(2, char::is_whitespace);
    let table = parts.next().unwrap_or("").trim();
    if table.is_empty() {
        return Err(ParseError::new(
            line,
            "expected `delete <table> [where <expr>]`",
        ));
    }
    let rest = parts.next().unwrap_or("").trim_start();
    let filter = if rest.is_empty() {
        None
    } else {
        let filter_text = rest
            .strip_prefix("where ")
            .ok_or_else(|| ParseError::new(line, "expected `where` after the delete table"))?;
        if filter_text.trim().is_empty() {
            return Err(ParseError::new(
                line,
                "expected an expression after `where`",
            ));
        }
        Some(parse_expr(filter_text, params, line)?)
    };

    Ok(Body::Delete {
        table: parse_ident(table).map_err(|message| ParseError::new(line, message))?,
        filter,
    })
}

fn require_no_terminal(terminal: &Option<Body>, line: &Line) -> Result<(), ParseError> {
    if terminal.is_some() {
        return Err(ParseError::new(
            line.no,
            "this statement cannot be combined with `count`, `update`, `delete`, or `insert`",
        ));
    }
    Ok(())
}

fn require_no_query_clauses(
    filter: &Option<Expr>,
    order: &[(String, bool)],
    limit: &Option<Limit>,
    offset: &Option<Limit>,
    line: &Line,
) -> Result<(), ParseError> {
    if filter.is_some() || !order.is_empty() || limit.is_some() || offset.is_some() {
        return Err(ParseError::new(
            line.no,
            "`count`, `update`, `delete`, and `insert` cannot be combined with query clauses",
        ));
    }
    Ok(())
}

fn parse_expr(text: &str, params: &HashSet<String>, line: usize) -> Result<Expr, ParseError> {
    let tokens = tokenize(text).map_err(|message| ParseError::new(line, message))?;
    if tokens.is_empty() {
        return Err(ParseError::new(line, "expected an expression"));
    }
    let mut parser = ExprParser {
        tokens,
        pos: 0,
        params,
    };
    let expr = parser
        .parse_or()
        .map_err(|message| ParseError::new(line, message))?;
    if parser.pos != parser.tokens.len() {
        return Err(ParseError::new(
            line,
            "unexpected text after the expression",
        ));
    }
    Ok(expr)
}

fn parse_operand_text(
    text: &str,
    params: &HashSet<String>,
    line: usize,
) -> Result<Operand, ParseError> {
    let tokens = tokenize(text).map_err(|message| ParseError::new(line, message))?;
    if tokens.len() != 1 {
        return Err(ParseError::new(
            line,
            "expected a field, parameter, or literal",
        ));
    }
    let mut parser = ExprParser {
        tokens,
        pos: 0,
        params,
    };
    parser
        .parse_operand()
        .map_err(|message| ParseError::new(line, message))
}

fn split_keyword<'a>(text: &'a str, keyword: &str) -> Option<(&'a str, &'a str)> {
    let mut in_string = false;
    let mut escaped = false;

    for (index, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            continue;
        }
        if text[index..].starts_with(keyword) {
            let before_ok = text[..index]
                .chars()
                .next_back()
                .is_none_or(|before| !is_ident_continue(before));
            let end = index + keyword.len();
            let after_ok = text[end..]
                .chars()
                .next()
                .is_none_or(|after| !is_ident_continue(after));
            if before_ok && after_ok {
                return Some((&text[..index], &text[end..]));
            }
        }
    }
    None
}

fn split_outside_strings(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_string = false;
    let mut escaped = false;

    for (index, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
        } else if ch == separator {
            parts.push(&text[start..index]);
            start = index + ch.len_utf8();
        }
    }
    parts.push(&text[start..]);
    parts
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Num(i128),
    Float(f64),
    Str(String),
    True,
    False,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    LParen,
    RParen,
    Dot,
}

struct ExprParser<'a> {
    tokens: Vec<Tok>,
    pos: usize,
    params: &'a HashSet<String>,
}

impl<'a> ExprParser<'a> {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }

    fn bump(&mut self) -> Option<Tok> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_and()?;
        while self.peek_is_keyword("or") {
            self.pos += 1;
            let right = self.parse_and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_primary()?;
        while self.peek_is_keyword("and") {
            self.pos += 1;
            let right = self.parse_primary()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        if matches!(self.peek(), Some(Tok::LParen)) {
            self.pos += 1;
            let inner = self.parse_or()?;
            match self.bump() {
                Some(Tok::RParen) => return Ok(inner),
                _ => return Err("expected `)`".to_string()),
            }
        }

        let left = self.parse_operand()?;
        let op = match self.bump() {
            Some(Tok::Eq) => Cmp::Eq,
            Some(Tok::Ne) => Cmp::Ne,
            Some(Tok::Lt) => Cmp::Lt,
            Some(Tok::Le) => Cmp::Le,
            Some(Tok::Gt) => Cmp::Gt,
            Some(Tok::Ge) => Cmp::Ge,
            _ => {
                return Err(
                    "expected a comparison operator (`==`, `!=`, `<`, `<=`, `>`, `>=`)".to_string(),
                )
            }
        };
        let right = self.parse_operand()?;
        Ok(Expr::Cmp { left, op, right })
    }

    fn parse_operand(&mut self) -> Result<Operand, String> {
        match self.bump() {
            Some(Tok::Ident(name)) => {
                if matches!(self.peek(), Some(Tok::Dot)) {
                    self.bump();
                    let field = match self.bump() {
                        Some(Tok::Ident(field)) => field,
                        _ => return Err("expected a field after `.`".to_string()),
                    };
                    if matches!(self.peek(), Some(Tok::Dot)) {
                        return Err("expected `alias.field`".to_string());
                    }
                    return Ok(Operand::Field(format!("{name}.{field}")));
                }
                if self.params.contains(&name) {
                    Ok(Operand::Param(name))
                } else {
                    Ok(Operand::Field(name))
                }
            }
            Some(Tok::Num(value)) => Ok(Operand::Literal(integer_literal(value)?)),
            Some(Tok::Float(value)) => Ok(Operand::Literal(Value::F64(value))),
            Some(Tok::Str(value)) => Ok(Operand::Literal(Value::Text(value))),
            Some(Tok::True) => Ok(Operand::Literal(Value::Bool(true))),
            Some(Tok::False) => Ok(Operand::Literal(Value::Bool(false))),
            _ => Err("expected a field, parameter, or literal".to_string()),
        }
    }

    fn peek_is_keyword(&self, keyword: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(name)) if name == keyword)
    }
}

fn tokenize(text: &str) -> Result<Vec<Tok>, String> {
    let mut tokens = Vec::new();
    let mut chars = text.char_indices().peekable();

    while let Some((_, ch)) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }
        match ch {
            '(' => tokens.push(Tok::LParen),
            ')' => tokens.push(Tok::RParen),
            '.' => tokens.push(Tok::Dot),
            '=' => {
                if matches!(chars.peek(), Some((_, '='))) {
                    chars.next();
                    tokens.push(Tok::Eq);
                } else {
                    return Err("use `==` for equality".to_string());
                }
            }
            '!' => {
                if matches!(chars.peek(), Some((_, '='))) {
                    chars.next();
                    tokens.push(Tok::Ne);
                } else {
                    return Err("use `!=` for inequality".to_string());
                }
            }
            '<' => {
                if matches!(chars.peek(), Some((_, '='))) {
                    chars.next();
                    tokens.push(Tok::Le);
                } else {
                    tokens.push(Tok::Lt);
                }
            }
            '>' => {
                if matches!(chars.peek(), Some((_, '='))) {
                    chars.next();
                    tokens.push(Tok::Ge);
                } else {
                    tokens.push(Tok::Gt);
                }
            }
            '"' => {
                let mut value = String::new();
                let mut closed = false;
                while let Some((_, next)) = chars.next() {
                    if next == '"' {
                        closed = true;
                        break;
                    }
                    if next == '\\' {
                        match chars.next() {
                            Some((_, 'n')) => value.push('\n'),
                            Some((_, 't')) => value.push('\t'),
                            Some((_, '"')) => value.push('"'),
                            Some((_, '\\')) => value.push('\\'),
                            Some((_, other)) => {
                                return Err(format!("unsupported string escape `\\{other}`"))
                            }
                            None => return Err("unterminated string".to_string()),
                        }
                    } else {
                        value.push(next);
                    }
                }
                if !closed {
                    return Err("unterminated string".to_string());
                }
                tokens.push(Tok::Str(value));
            }
            '-' | '0'..='9' => {
                let mut number = String::new();
                number.push(ch);
                while let Some((_, next)) = chars.peek().copied() {
                    if next.is_ascii_digit() {
                        number.push(next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if number == "-" {
                    return Err("expected digits after `-`".to_string());
                }
                let mut is_float = false;
                if matches!(chars.peek(), Some((_, '.'))) {
                    let mut lookahead = chars.clone();
                    lookahead.next();
                    if matches!(lookahead.next(), Some((_, digit)) if digit.is_ascii_digit()) {
                        chars.next();
                        number.push('.');
                        is_float = true;
                        while let Some((_, next)) = chars.peek().copied() {
                            if next.is_ascii_digit() {
                                number.push(next);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    }
                }
                if matches!(chars.peek(), Some((_, 'e' | 'E'))) {
                    let mut lookahead = chars.clone();
                    lookahead.next();
                    let mut exponent = String::new();
                    if matches!(lookahead.peek(), Some((_, '+' | '-'))) {
                        if let Some((_, sign)) = lookahead.next() {
                            exponent.push(sign);
                        }
                    }
                    if matches!(lookahead.next(), Some((_, digit)) if digit.is_ascii_digit()) {
                        chars.next();
                        number.push('e');
                        if !exponent.is_empty() {
                            chars.next();
                            number.push(exponent.as_bytes()[0] as char);
                        }
                        while let Some((_, next)) = chars.peek().copied() {
                            if next.is_ascii_digit() {
                                number.push(next);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        is_float = true;
                    }
                }
                if is_float {
                    let value: f64 = number
                        .parse()
                        .map_err(|_| format!("number `{number}` is out of range"))?;
                    tokens.push(Tok::Float(value));
                } else {
                    let value: i128 = number
                        .parse()
                        .map_err(|_| format!("number `{number}` is out of range"))?;
                    integer_literal(value)?;
                    tokens.push(Tok::Num(value));
                }
            }
            _ if is_ident_start(ch) => {
                let mut name = String::new();
                name.push(ch);
                while let Some((_, next)) = chars.peek().copied() {
                    if is_ident_continue(next) {
                        name.push(next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                match name.as_str() {
                    "true" => tokens.push(Tok::True),
                    "false" => tokens.push(Tok::False),
                    _ => tokens.push(Tok::Ident(name)),
                }
            }
            other => return Err(format!("unexpected character `{other}`")),
        }
    }

    Ok(tokens)
}

pub fn parse_ident(text: &str) -> Result<String, String> {
    if text.is_empty() {
        return Err("expected a name".to_string());
    }
    let mut chars = text.chars();
    let first = chars.next().unwrap();
    if !is_ident_start(first) {
        return Err(format!("`{text}` is not a valid name"));
    }
    if !chars.all(is_ident_continue) {
        return Err(format!("`{text}` is not a valid name"));
    }
    Ok(text.to_string())
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn parse_type(text: &str) -> Result<Ty, String> {
    Ty::parse(text).ok_or_else(|| {
        format!(
            "unknown type `{text}` (supported: i8, i16, i32, i64, u8, u16, u32, u64, f32, f64, bool, text, bytes, uuid)"
        )
    })
}

fn coerce_literal(value: Value, ty: Ty) -> Option<Value> {
    value.coerce_to(ty)
}

fn integer_literal(value: i128) -> Result<Value, String> {
    if let Ok(value) = i64::try_from(value) {
        Ok(Value::I64(value))
    } else if let Ok(value) = u64::try_from(value) {
        Ok(Value::U64(value))
    } else {
        Err(format!("number `{value}` is out of range"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::validate::validate;

    const SOURCE: &str = include_str!("../../../examples/messages.lightshade");

    #[test]
    fn parses_the_messages_workload() {
        let workload = parse_workload(SOURCE).expect("example should parse");
        assert_eq!(workload.tables.len(), 1);
        assert_eq!(workload.operations.len(), 4);

        let messages = workload.table("messages").unwrap();
        assert_eq!(messages.fields.len(), 5);
        assert!(messages.primary_key().unwrap().name == "id");

        let recent = workload.operation("recent_messages").unwrap();
        match &recent.body {
            Body::Query {
                table,
                filter,
                order,
                limit,
                count,
                ..
            } => {
                assert_eq!(table, "messages");
                assert!(filter.is_some());
                assert_eq!(
                    order,
                    &vec![("sent_at".to_string(), true), ("id".to_string(), true)]
                );
                assert!(matches!(limit, Some(Limit::Param(name)) if name == "limit"));
                assert!(!count);
            }
            other => panic!("unexpected body: {other:?}"),
        }

        let count = workload.operation("unread_count").unwrap();
        match &count.body {
            Body::Query { count, table, .. } => {
                assert!(*count);
                assert_eq!(table, "messages");
            }
            other => panic!("unexpected body: {other:?}"),
        }
    }

    #[test]
    fn parses_projections_aggregates_and_joins() {
        let workload =
            parse_workload(include_str!("../../../examples/inbox_digest.lightshade")).unwrap();
        let digest = workload.operation("unread_digest").unwrap();
        match &digest.body {
            Body::Query {
                table,
                alias,
                joins,
                select,
                group_by,
                ..
            } => {
                assert_eq!(table, "messages");
                assert_eq!(alias.as_deref(), Some("m"));
                assert_eq!(joins.len(), 1);
                assert_eq!(joins[0].table, "inboxes");
                assert_eq!(joins[0].alias.as_deref(), Some("i"));
                assert_eq!(
                    joins[0].keys[0].left,
                    FieldRef::parse("m.inbox_id").unwrap()
                );
                assert_eq!(joins[0].keys[0].right, FieldRef::parse("i.id").unwrap());
                assert_eq!(select.len(), 5);
                assert_eq!(select[0].alias, "owner");
                assert!(matches!(
                    select[1].expr,
                    SelectExpr::Aggregate {
                        function: Aggregate::Count,
                        field: None
                    }
                ));
                assert_eq!(group_by, &[FieldRef::parse("i.owner").unwrap()]);
            }
            other => panic!("unexpected body: {other:?}"),
        }

        let overall = workload.operation("overall").unwrap();
        match &overall.body {
            Body::Query {
                select,
                group_by,
                joins,
                ..
            } => {
                assert!(joins.is_empty());
                assert!(group_by.is_empty());
                assert_eq!(select.len(), 5);
            }
            other => panic!("unexpected body: {other:?}"),
        }
    }

    #[test]
    fn aggregate_select_items_need_an_output_name() {
        let source = r#"
            table notes {
                id: i64 primary_key
            }
            table out {
                total: i64 primary_key
            }
            operation bad() -> many out {
                from notes
                select count()
            }
        "#;
        let error = parse_workload(source).unwrap_err();
        assert!(error.message.contains("need `as <name>`"), "{error}");
    }

    #[test]
    fn parses_quoted_strings_and_comments() {
        let source = r#"
            table notes {
                id: i64 primary_key
                body: text
            }
            operation by_body(body: text) -> many notes {
                filter body == body   # trailing comment
            }
        "#;
        let workload = parse_workload(source).expect("should parse");
        assert_eq!(workload.operations.len(), 1);
    }

    #[test]
    fn parses_float_literals_and_defaults() {
        assert_eq!(tokenize("1e2").unwrap(), vec![Tok::Float(100.0)]);
        assert_eq!(tokenize("-1.5e-2").unwrap(), vec![Tok::Float(-0.015)]);
        let source = r#"
            table samples {
                id: i64 primary_key
                value: f64 default 1.5e2
                score: f32 default 0.5
            }
            operation all() -> many samples {
                filter value >= 1e2 and score <= 0.5
            }
        "#;
        let workload = parse_workload(source).unwrap();
        let table = workload.table("samples").unwrap();
        assert_eq!(table.fields[1].default, Some(Value::F64(150.0)));
        assert_eq!(table.fields[2].default, Some(Value::F32(0.5)));
    }

    #[test]
    fn parses_full_range_integer_literals() {
        assert_eq!(
            integer_literal(9_223_372_036_854_775_807).unwrap(),
            Value::I64(i64::MAX)
        );
        assert_eq!(
            integer_literal(9_223_372_036_854_775_808).unwrap(),
            Value::U64(9_223_372_036_854_775_808)
        );
        assert_eq!(
            integer_literal(18_446_744_073_709_551_615).unwrap(),
            Value::U64(u64::MAX)
        );
        let source = r#"
            table counters {
                id: i64 primary_key
                high: u64 default 18446744073709551615
            }
        "#;
        let workload = parse_workload(source).unwrap();
        assert_eq!(
            workload.table("counters").unwrap().fields[1].default,
            Some(Value::U64(u64::MAX))
        );
        let too_big = r#"
            table counters {
                id: i64 primary_key
                high: u64 default 18446744073709551616
            }
        "#;
        let error = parse_workload(too_big).unwrap_err();
        assert!(error.message.contains("out of range"), "{}", error.message);
    }

    #[test]
    fn rejects_unknown_statement() {
        let source = r#"
            operation nope() {
                frobnicate messages
            }
        "#;
        let error = parse_workload(source).unwrap_err();
        assert!(error.message.contains("unknown operation statement"));
    }

    #[test]
    fn parses_offset_and_rejects_misplaced_clauses() {
        let source = r#"
table notes {
    id: i64 primary_key
    rank: i64
}

operation page(limit: i64, skip: i64) -> many notes {
    order by rank asc
    limit limit
    offset skip
}

operation tail() -> many notes {
    order by rank asc
    limit 2
    offset 1
}
"#;
        let workload = parse_workload(source).unwrap();
        validate(&workload).unwrap();
        let page = workload.operation("page").unwrap();
        match &page.body {
            Body::Query { limit, offset, .. } => {
                assert!(matches!(limit, Some(Limit::Param(name)) if name == "limit"));
                assert!(matches!(offset, Some(Limit::Param(name)) if name == "skip"));
            }
            other => panic!("unexpected body: {other:?}"),
        }
        let tail = workload.operation("tail").unwrap();
        match &tail.body {
            Body::Query { limit, offset, .. } => {
                assert!(matches!(limit, Some(Limit::Literal(2))));
                assert!(matches!(offset, Some(Limit::Literal(1))));
            }
            other => panic!("unexpected body: {other:?}"),
        }

        let duplicated = r#"
table notes {
    id: i64 primary_key
}

operation bad() -> many notes {
    offset 1
    offset 2
}
"#;
        let error = parse_workload(duplicated).unwrap_err();
        assert!(error.message.contains("duplicate `offset` clause"));

        let counted = r#"
table notes {
    id: i64 primary_key
}

operation bad() -> one i64 {
    offset 1
    count notes where id == 1
}
"#;
        let error = parse_workload(counted).unwrap_err();
        assert!(error
            .message
            .contains("cannot be combined with query clauses"));

        let unvalidated = r#"
table notes {
    id: i64 primary_key
}

operation bad(skip: i64) -> many notes {
    offset skip
    limit skip
}
"#;
        let workload = parse_workload(unvalidated).unwrap();
        assert!(validate(&workload).is_ok());

        let mistyped = r#"
table notes {
    id: i64 primary_key
}

operation bad(skip: text) -> many notes {
    offset skip
}
"#;
        let workload = parse_workload(mistyped).unwrap();
        let error = validate(&workload).unwrap_err();
        assert!(error.contains("offset parameter `skip` must be `i64`"));
    }

    #[test]
    fn parses_field_defaults_and_rejects_mismatches() {
        let source = r#"
            table notes {
                id: i64 primary_key
                tag: text default "misc"
                weight: i64 default -7
                pinned: bool default true
            }
            operation add(id: i64, tag: text, weight: i64, pinned: bool) {
                insert notes
            }
        "#;
        let workload = parse_workload(source).expect("should parse");
        let notes = workload.table("notes").unwrap();
        assert_eq!(notes.fields[0].default, None);
        assert_eq!(
            notes.fields[1].default,
            Some(Value::Text("misc".to_string()))
        );
        assert_eq!(notes.fields[2].default, Some(Value::I64(-7)));
        assert_eq!(notes.fields[3].default, Some(Value::Bool(true)));

        let mismatch = r#"
            table notes {
                id: i64 primary_key
                weight: i64 default "heavy"
            }
        "#;
        let error = parse_workload(mismatch).unwrap_err();
        assert!(
            error
                .message
                .contains("default value does not match field type"),
            "{}",
            error.message
        );

        let missing = r#"
            table notes {
                id: i64 primary_key
                tag: text default
            }
        "#;
        let error = parse_workload(missing).unwrap_err();
        assert!(
            error.message.contains("expected a value after `default`"),
            "{}",
            error.message
        );

        let two = r#"
            table notes {
                id: i64 primary_key
                tag: text default "a" "b"
            }
        "#;
        let error = parse_workload(two).unwrap_err();
        assert!(
            error
                .message
                .contains("expected a single literal after `default`"),
            "{}",
            error.message
        );
    }
}
