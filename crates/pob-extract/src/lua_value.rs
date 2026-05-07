//! Tiny helpers for picking values out of `serde_json::Value`s produced by `lua_to_json`.

#![allow(dead_code)]

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;

pub fn get<'a>(o: &'a Value, key: &str) -> Option<&'a Value> {
    o.as_object().and_then(|m| m.get(key))
}

pub fn require<'a>(o: &'a Value, key: &str) -> Result<&'a Value> {
    get(o, key).ok_or_else(|| anyhow!("missing field `{key}`"))
}

pub fn opt_str(o: &Value, key: &str) -> Option<String> {
    get(o, key).and_then(|v| v.as_str().map(str::to_owned))
}

pub fn opt_f64(o: &Value, key: &str) -> Option<f64> {
    get(o, key).and_then(serde_json::Value::as_f64)
}

pub fn opt_u64(o: &Value, key: &str) -> Option<u64> {
    get(o, key).and_then(serde_json::Value::as_u64)
}

pub fn opt_i64(o: &Value, key: &str) -> Option<i64> {
    get(o, key).and_then(serde_json::Value::as_i64)
}

pub fn opt_bool(o: &Value, key: &str) -> Option<bool> {
    get(o, key).and_then(serde_json::Value::as_bool)
}

pub fn req_str(o: &Value, key: &str) -> Result<String> {
    require(o, key)?
        .as_str()
        .map(str::to_owned)
        .with_context(|| format!("`{key}` is not a string"))
}

pub fn req_i64(o: &Value, key: &str) -> Result<i64> {
    let v = require(o, key)?;
    v.as_i64()
        .or_else(|| v.as_f64().map(|n| n as i64))
        .with_context(|| format!("`{key}` is not a number"))
}

pub fn req_f64(o: &Value, key: &str) -> Result<f64> {
    let v = require(o, key)?;
    v.as_f64()
        .with_context(|| format!("`{key}` is not a number"))
}

pub fn req_array<'a>(o: &'a Value, key: &str) -> Result<&'a [Value]> {
    require(o, key)?
        .as_array()
        .map(Vec::as_slice)
        .with_context(|| format!("`{key}` is not an array"))
}

pub fn opt_array<'a>(o: &'a Value, key: &str) -> Result<&'a [Value]> {
    Ok(get(o, key)
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]))
}

pub fn req_object<'a>(
    o: &'a Value,
    key: &str,
) -> Result<&'a serde_json::Map<String, Value>> {
    require(o, key)?
        .as_object()
        .with_context(|| format!("`{key}` is not an object"))
}

pub fn opt_object<'a>(
    o: &'a Value,
    key: &str,
) -> Option<&'a serde_json::Map<String, Value>> {
    get(o, key).and_then(Value::as_object)
}

pub fn parse_id(s: &str) -> Result<u32> {
    s.parse::<u32>()
        .map_err(|e| anyhow!("invalid numeric id {s}: {e}"))
}

#[allow(dead_code)]
pub fn ensure_object(v: &Value) -> Result<&serde_json::Map<String, Value>> {
    v.as_object().ok_or_else(|| anyhow!("expected object"))
}

#[allow(dead_code)]
pub fn ensure_array(v: &Value) -> Result<&[Value]> {
    match v.as_array() {
        Some(a) => Ok(a.as_slice()),
        None => bail!("expected array, got {v}"),
    }
}
