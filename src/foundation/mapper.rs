#![allow(clippy::empty_docs)]

use crate::foundation::filter::{ValueProvider};
use crate::foundation::mapper::EvalResult::{Named, Value, Undefined, Failure, AnyValue};
use crate::model::ItemField;
use crate::tuliprox_error::{create_tuliprox_error_result, info_err, TuliproxError, TuliproxErrorKind};
use log::error;
use pest::iterators::Pair;
use pest::Parser;
use regex::{Regex};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use crate::utils::Capitalize;

#[derive(Parser)]
#[grammar_inline = r##"
WHITESPACE = _{ " " | "\t"}
regex_op =  _{ "~" }
identifier = @{ (ASCII_ALPHANUMERIC | "_")+ }
string_literal = @{ "\"" ~ ( "\\\"" | (!"\"" ~ ANY) )* ~ "\"" }
field = { ^"group" | ^"title" | ^"name" | ^"url" | ^"input" | ^"caption"}
regex_expr = { field ~ regex_op ~ string_literal }
expression = _{ match_block | function_call | regex_expr | string_literal | identifier }
function_name = {  ^"concat" | ^"uppercase" | ^"lowercase" | ^"capitalize" | ^"trim"}
function_call = { function_name ~ "(" ~ (expression ~ ("," ~ expression)*)? ~ ")" }
any_match = { "_" }
match_key = { any_match | identifier }
match_key_list = { match_key ~ ("," ~ match_key)* }
match_case = { match_key_list ~ "=>" ~ expression | "(" ~ match_key_list ~ ")" ~ "=>" ~ expression }
match_block = { "match" ~  "{" ~ NEWLINE* ~ (match_case ~ ("," ~ NEWLINE* ~ match_case)*)? ~ ","? ~ NEWLINE* ~ "}" }
map_case_key = { any_match | string_literal}
map_case = { map_case_key ~ "=>" ~ expression }
map_key = { identifier }
map_block = { "map" ~ map_key ~ "{" ~ NEWLINE* ~ (map_case ~ ("," ~ NEWLINE* ~ map_case)*)? ~ ","? ~ NEWLINE* ~ "}" }
assignment = { (field | identifier) ~ "=" ~ expression }
statement = { assignment | expression }
comment = _{ "#" ~ (!NEWLINE ~ ANY)* }
statement_reparator = _{ ";" | NEWLINE }
statements = _{ (statement_reparator* ~ (statement | comment))* ~ statement_reparator* }
main = { SOI ~ statements? ~ EOI }
"##]

struct MapperParser;

#[derive(Debug, Clone)]
enum MatchCaseKey {
    Identifier(String),
    String(String),
    AnyMatch,
}

#[derive(Debug, Clone)]
struct MatchCase {
    pub identifiers: Vec<MatchCaseKey>,
    pub expression: Expression,
}

#[derive(Debug, Clone)]
enum MatchKey {
    Identifier(String),
}


#[derive(Debug, Clone)]
enum BuiltInFunction {
    Concat,
    Uppercase,
    Lowercase,
    Capitalize,
    Trim,
}

impl FromStr for BuiltInFunction {
    type Err = TuliproxError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "concat" => Ok(Self::Concat),
            "capitalize" => Ok(Self::Capitalize),
            "lowercase" => Ok(Self::Lowercase),
            "uppercase" => Ok(Self::Uppercase),
            "trim" => Ok(Self::Trim),
            _ => create_tuliprox_error_result!(TuliproxErrorKind::Info, "Unknown function {}", s),
        }
    }
}

#[derive(Debug, Clone)]
enum Expression {
    Identifier(String),
    StringLiteral(String),
    RegexExpr { field: ItemField, pattern: String, re_pattern: Regex },
    FunctionCall { name: BuiltInFunction, args: Vec<Expression> },
    MatchBlock { keys: Vec<MatchKey>, cases: Vec<MatchCase> },
}

#[derive(Debug, Clone)]
enum AssignmentTarget {
    Identifier(String),
    Field(ItemField),
}

#[derive(Debug, Clone)]
enum Statement {
    Assignment { target: AssignmentTarget, expr: Expression },
    Expression(Expression),
    Comment(String),
}

#[derive(Debug)]
pub struct MapperProgram {
    statements: Vec<Statement>,
}

impl Statement {
    pub fn eval(&self, ctx: &mut Context, provider: &ValueProvider) -> Result<(), TuliproxError> {
        match self {
            Statement::Assignment { target, expr } => {
                let val = expr.eval(ctx, provider);
                match target {
                    AssignmentTarget::Identifier(name) => {
                        ctx.variables.insert(name.clone(), val);
                    }
                    AssignmentTarget::Field(name) => {
                        // TODO set fiels value
                        error!("Set field value not implemented yet. {name} = {val:?}");
                    }
                }
            }
            Statement::Expression(expr) => {
                let result = expr.eval(ctx, provider);
                error!("Ignoring result {result:?}");
            }
            Statement::Comment(_) => {}
        }
        Ok(())
    }
}

impl MapperProgram {
    fn validate_expr<'a>(expr: &Expression, identifiers: &mut HashSet<&'a str>) -> Result<(), TuliproxError> {
        match expr {
            Expression::Identifier(ident) => {
                if !identifiers.contains(ident.as_str()) {
                    return create_tuliprox_error_result!(TuliproxErrorKind::Info, "Identifier unknown {}", ident);
                }
            }
            Expression::StringLiteral(_) => {}
            Expression::RegexExpr { field: _field, pattern: _pattern, re_pattern: _re_pattern } => {}
            Expression::FunctionCall { name: _name, args } => {
                for arg in args {
                    MapperProgram::validate_expr(arg, identifiers)?;
                }
            }
            Expression::MatchBlock {keys, cases} => {
                let mut key_count = 0;
                for key in keys {
                    match key {
                        MatchKey::Identifier(ident) => {
                            if !identifiers.contains(ident.as_str()) {
                                return create_tuliprox_error_result!(TuliproxErrorKind::Info, "Identifier unknown {}", ident);
                            }
                            key_count += 1;
                        }
                    }
                }
                for match_case in cases {
                    let mut any_match_count = 0;
                    if match_case.identifiers.len() != key_count {
                        return create_tuliprox_error_result!(TuliproxErrorKind::Info, "Match key count does not match case key count");
                    }
                    for identifier in &match_case.identifiers {
                        match identifier {
                            MatchCaseKey::Identifier(ident) => {
                                if !identifiers.contains(ident.as_str()) {
                                    return create_tuliprox_error_result!(TuliproxErrorKind::Info, "Identifier unknown {}", ident);
                                }
                            }
                            MatchCaseKey::String(_) => {}
                            MatchCaseKey::AnyMatch => {
                                any_match_count += 1;
                                if any_match_count > 1 {
                                    return create_tuliprox_error_result!(TuliproxErrorKind::Info, "Match arm can only have one '_'");
                                }
                            }
                        }
                    }
                    MapperProgram::validate_expr(&match_case.expression, identifiers)?;
                }
            }
        }
        Ok(())
    }

    fn validate(statements: &Vec<Statement>) -> Result<(), TuliproxError> {
        let mut identifiers: HashSet<&str> = HashSet::new();
        for stmt in statements {
            match stmt {
                Statement::Assignment { target, expr: value } => {
                    match target {
                        AssignmentTarget::Identifier(ident) => {
                            identifiers.insert(ident.as_str());
                        }
                        AssignmentTarget::Field(_) => {}
                    }
                    MapperProgram::validate_expr(value, &mut identifiers)?;
                }
                Statement::Expression(expr) => {
                    MapperProgram::validate_expr(expr, &mut identifiers)?;
                }
                Statement::Comment(_) => {}
            }
        }
        Ok(())
    }

    pub fn parse(input: &str) -> Result<Self, TuliproxError> {
        let mut parsed = MapperParser::parse(Rule::main, input).map_err(|e| info_err!(e.to_string()))?;
        let program_pair = parsed.next().unwrap();
        let mut statements = Vec::new();
        for stmt_pair in program_pair.into_inner() {
            if let Some(stmt) = Self::parse_statement(stmt_pair)? {
                statements.push(stmt);
            }
        }
        MapperProgram::validate(&statements)?;
        Ok(Self { statements })
    }
    fn parse_statement(pair: Pair<Rule>) -> Result<Option<Statement>, TuliproxError> {
        match pair.as_rule() {
            Rule::statement => {
                let inner = pair.into_inner().next().unwrap();
                match inner.as_rule() {
                    Rule::assignment => Ok(Some(MapperProgram::parse_assignment(inner)?)),
                    Rule::expression => Ok(Some(Statement::Expression(MapperProgram::parse_expression(inner)?))),
                    _ => Ok(None),
                }
            }
            Rule::comment => Ok(Some(Statement::Comment(pair.as_str().trim().to_string()))),
            _ => Ok(None),
        }
    }

    fn parse_assignment(pair: Pair<Rule>) -> Result<Statement, TuliproxError> {
        let mut inner = pair.into_inner();
        let name = inner.next().unwrap();
        let target = match name.as_rule() {
            Rule::identifier => AssignmentTarget::Identifier(name.as_str().to_string()),
            Rule::field => AssignmentTarget::Field(ItemField::from_str(name.as_str())?),
            _ => return create_tuliprox_error_result!(TuliproxErrorKind::Info, "Assignment target not supported {}", name.as_str()),
        };
        let next = inner.next().unwrap();
        let value = MapperProgram::parse_expression(next)?;
        Ok(Statement::Assignment { target, expr: value })
    }

    fn parse_match_case_key(pair: Pair<Rule>) -> Result<MatchCaseKey, TuliproxError> {
        let mut inner = pair.into_inner().next().unwrap();
        match inner.as_rule() {
            Rule::identifier => Ok(MatchCaseKey::Identifier(inner.as_str().to_string())),
            Rule::string_literal => Ok(MatchCaseKey::String(inner.as_str().to_string())),
            Rule::any_match => Ok(MatchCaseKey::AnyMatch),
            _ => create_tuliprox_error_result!(TuliproxErrorKind::Info, "Unexpected match_key: {:?}", inner.as_rule()),
        }
    }

    fn parse_match_case(pair: Pair<Rule>) -> Result<MatchCase, TuliproxError> {
        let mut inner = pair.into_inner();

        let first = inner.next().unwrap();

        let identifiers = match first.as_rule() {
            Rule::case_key => {
                vec![MapperProgram::parse_match_case_key(first)?]
            }
            Rule::case_key_list => {
                let mut matches = vec![];
                for arm in first.into_inner() {
                    if arm.as_rule() != Rule::WHITESPACE {
                        matches.push(MapperProgram::parse_match_case_key(arm)?);
                    }
                }
                matches
            }
            _ => return create_tuliprox_error_result!(TuliproxErrorKind::Info, "Unexpected match arm input: {:?}", first.as_rule()),
        };

        let expr = MapperProgram::parse_expression(inner.next().unwrap())?;

        Ok(MatchCase {
            identifiers,
            expression: expr,
        })
    }

    fn parse_expression(pair: Pair<Rule>) -> Result<Expression, TuliproxError> {
        match pair.as_rule() {
            Rule::identifier => Ok(Expression::Identifier(pair.as_str().to_string())),

            Rule::string_literal => {
                let raw = pair.as_str();
                // remove quotes
                let content = &raw[1..raw.len() - 1];
                Ok(Expression::StringLiteral(content.to_string()))
            }

            Rule::regex_expr => {
                let mut inner = pair.into_inner();
                let field = ItemField::from_str(inner.next().unwrap().as_str())?;
                let pattern_raw = inner.next().unwrap().as_str();
                let pattern = &pattern_raw[1..pattern_raw.len() - 1]; // Strip quotes
                match Regex::new(pattern) {
                    Ok(re) => Ok(Expression::RegexExpr { field, pattern: pattern.to_string(), re_pattern: re }),
                    Err(_) => create_tuliprox_error_result!(TuliproxErrorKind::Info, "Invalid regex {}", pattern),
                }
            }

            Rule::function_call => {
                let mut inner = pair.into_inner();
                let fn_name = inner.next().unwrap().as_str().to_string();
                let mut args = vec![];
                for arg in inner {
                    args.push(MapperProgram::parse_expression(arg)?);
                }
                let name = BuiltInFunction::from_str(&fn_name)?;
                Ok(Expression::FunctionCall { name, args })
            }

            Rule::match_block => {
                let case_pairs = pair.into_inner();
                let mut cases = vec![];
                for arm in case_pairs {
                    cases.push(MapperProgram::parse_match_case(arm)?);
                }
                Err(info_err!("Failed".to_string()))
                //Ok(Expression::MatchBlock { cases})
            }

            _ => create_tuliprox_error_result!(TuliproxErrorKind::Info, "Unknown expression rule: {:?}", pair.as_rule()),
        }
    }
}

pub struct Context {
    variables: HashMap<String, EvalResult>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
        }
    }

    pub fn add_var(&mut self, name: &str, value: EvalResult) {
        self.variables.insert(name.to_string(), value);
    }

    pub fn has_var(&self, name: &str) -> bool {
        self.variables.contains_key(name)
    }

    pub fn get_var(&self, name: &str) -> &EvalResult {
        self.variables.get(name).unwrap_or(&Undefined)
    }

}

#[derive(Debug)]
#[derive(Clone)]
enum EvalResult {
    Undefined,
    Value(String),
    Named(Vec<(String, String)>),
    AnyValue,
    Failure(String),
}

impl EvalResult {
    fn matches(&self, other: &EvalResult) -> bool {
        match (self, other) {
            (AnyValue, _)
            | (_, AnyValue) => true,
            (Value(a), Value(b)) => a == b,
            (Named(a), Named(b)) => a == b, // Oder eigene Logik!
            (Failure(_), _) | (_, Failure(_)) => false,
            (Undefined, _) | (_, Undefined) => false,
            _ => false,
        }
    }
    pub fn is_error(&self) -> bool {
        match self {
            Failure(_) => true,
            _ => false,
        }
    }
}

fn concat_args(args: &Vec<EvalResult>) -> Vec<&str> {
    let mut result = vec![];

    for arg in args {
        match arg {
            Undefined => {}
            Value(value) => result.push(value.as_str()),
            Named(pairs) => {
                for (i, (key, value)) in pairs.iter().enumerate() {
                    result.push(key.as_str());
                    result.push(": ");
                    result.push(value.as_str());
                    if i < pairs.len() - 1 {
                        result.push(", ");
                    }
                }
            }
            AnyValue => {}
            Failure(_) => {}
        }
    }

    result
}

impl Expression {

    pub fn eval(&self, ctx: &mut Context, provider: &ValueProvider) -> EvalResult {
        match self {
            Expression::Identifier(name) => {
                match ctx.variables.get(name) {
                    None => EvalResult::Failure(format!("Variable with name {name} not found.")),
                    Some(value) => value.clone(),
                }
            }
            Expression::StringLiteral(s) => Value(s.clone()),
            Expression::RegexExpr { field, pattern, re_pattern } => {
                let val = provider.call(field);
                let mut values = vec![];
                for caps in re_pattern.captures_iter(&val) {
                    for name in re_pattern.capture_names().flatten() {
                        if let Some(m) = caps.name(name) {
                            values.push((name.to_string(), m.as_str().to_string()));
                        }
                    }
                }
                if values.is_empty() {
                    return Undefined;
                }
                Named(values)
            }
            Expression::FunctionCall { name, args } => {
                let evaluated_args: Vec<EvalResult> = args.iter().map(|a| a.eval(ctx, provider)).collect();
                for arg in &evaluated_args {
                    if arg.is_error() {
                        return arg.clone();
                    }
                }

                match name {
                    BuiltInFunction::Concat => Value(concat_args(&evaluated_args).join("")),
                    BuiltInFunction::Uppercase => Value(concat_args(&evaluated_args).join(" ").to_uppercase()),
                    BuiltInFunction::Trim => Value(concat_args(&evaluated_args).iter().map(|&s| s.trim()).collect::<Vec<_>>().join(" ").trim().to_string()),
                    BuiltInFunction::Lowercase => Value(concat_args(&evaluated_args).join(" ").to_lowercase()),
                    BuiltInFunction::Capitalize => Value(concat_args(&evaluated_args).iter().map(|&s| s.capitalize()).collect::<Vec<_>>().join(" ")),
                }
            }
            Expression::MatchBlock{ keys, cases} => {
                let mut match_keys = vec![];
                for key in keys {
                    match key {
                        MatchKey::Identifier(ident) => {
                            if !ctx.has_var(ident) {
                                return EvalResult::Failure(format!("Match expression invalid! Variable with name {ident} not found."));
                            }
                            match_keys.push(ctx.get_var(ident));
                        }
                    }
                }

                for match_case in cases {
                    let mut case_keys = vec![];
                    for case_key in &match_case.identifiers{
                        match case_key {
                            MatchCaseKey::Identifier(ident) => {
                                if !ctx.has_var(&ident) {
                                    return Failure(format!("Match case invalid! Variable with name {ident} not found."));
                                }
                                case_keys.push(ctx.get_var(&ident).clone());
                            }
                            MatchCaseKey::String(value) => case_keys.push(EvalResult::Value(value.to_string())),
                            MatchCaseKey::AnyMatch  => case_keys.push(AnyValue),
                        }
                    }

                    let mut match_count = 0;
                    for (case_key, &match_key) in case_keys.iter().zip(&match_keys) {
                        if !match_key.matches(case_key) {
                            match_count += 1;
                        }
                    }
                    if match_count == case_keys.len() {
                        return match_case.expression.eval(ctx, provider);
                    }
                }
                Undefined
            }
        }
    }
}
//
// pub fn eval_expression(expr: &Expression, ctx: &Context) -> Option<String> {
//     match expr {
//         Expression::StringLiteral(s) => Some(s.clone()),
//         Expression::Identifier(name) => ctx.get(name),
//         Expression::FunctionCall { name, args } => {
//             let eval_args: Vec<String> = args.iter()
//                 .filter_map(|a| eval_expression(a, ctx))
//                 .collect();
//             match name.as_str() {
//                 "uppercase" => eval_args.get(0).map(|s| s.to_uppercase()),
//                 "lowercase" => eval_args.get(0).map(|s| s.to_lowercase()),
//                 "capitalize" => eval_args.get(0).map(|s| {
//                     let mut c = s.chars();
//                     match c.next() {
//                         None => String::new(),
//                         Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
//                     }
//                 }),
//                 "concat" => Some(eval_args.join("")),
//                 _ => None,
//             }
//         }
//     }
// }
//
// pub fn eval_statement(stmt: &Statement, ctx: &mut Context) {
//     match stmt {
//         Statement::Assignment { target, value } => {
//             if let Some(val) = eval_expression(value, ctx) {
//                 ctx.set(target, val);
//             }
//         }
//         Statement::MatchAssignment { target, arms } => {
//             let values: Vec<Option<String>> = arms[0].pattern.iter().map(|v| ctx.get(v)).collect();
//             for arm in arms {
//                 let match_ok = arm.pattern.iter().enumerate().all(|(i, name)| {
//                     name == "_" || ctx.get(name).is_some()
//                 });
//                 if match_ok {
//                     if let Some(result) = eval_expression(&arm.result, ctx) {
//                         ctx.set(target, result);
//                         break;
//                     }
//                 }
//             }
//         }
//     }
// }
//
// pub fn eval_program(prog: &Program, ctx: &mut Context) {
//     for stmt in &prog.statements {
//         eval_statement(stmt, ctx);
//     }
// }


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mapper_dsl_eval() {
        let dsl = r#"
        coast = Caption ~ "(?i)(East|West)"
        quality = Caption ~ "(?i)(HD|FHD|LHD)"
        quality = uppercase(quality)
        quality = map quality {
                   "LHD" => "HD",
                   "SHD" => "SD",
                    _ => quality,
             },
             _ => quality
        }

        coast_quality = match {
            (coast, quality) => concat(capitalize(coast), " ", uppercase(quality)),
            (coast, _) => concat(capitalize(coast), " HD"),
            (_, quality) => concat("East ", uppercase(quality)),
        }

        result = concat("US: TNT ", coast_quality)
    "#;

        let mut program = MapperProgram::parse(dsl).expect("Parsing failed");
        println!("Program: {program:?}");

        // let mut ctx = Context::new();
        // // Beispiel-Felder für Caption
        // ctx.fields.insert("Caption".to_string(), "US: TNT East LHD bubble".to_string());
        //
        // for stmt in &program.statements {
        //     //let res = stmt.eval(&mut ctx);
        //     println!("Statement Result: {:?}", res);
        // }
        //
        // println!("Result variable: {:?}", ctx.variables.get("result"));
        // assert_eq!(ctx.variables.get("result").unwrap(), "US: TNT East HD");
    }
}
