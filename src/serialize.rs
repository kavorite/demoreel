use arrow2::chunk::Chunk;
use polars::prelude::DataFrame;
use pyo3::{
    types::{PyDict, PyList},
    IntoPy, PyObject, Python,
};
use serde::Serialize;
use serde_arrow::arrow2::{serialize_into_arrays, serialize_into_fields};
use serde_arrow::schema::TracingOptions;
use serde_json_path::JsonPath;

use crate::errors::*;

pub fn json_to_py<'py>(py: Python<'py>, v: &serde_json::Value) -> Result<PyObject> {
    use serde_json::Value;
    match v {
        Value::Object(obj) => {
            let dict = PyDict::new(py);
            for (k, v) in obj.into_iter() {
                dict.set_item(k, json_to_py(py, v)?)?;
            }
            Ok(dict.into())
        }
        Value::Array(arr) => {
            let istrm: Result<Vec<_>> = arr.into_iter().map(|x| json_to_py(py, x)).collect();
            Ok(PyList::new(py, istrm?).into())
        }
        Value::Null => Ok(py.None()),
        Value::Bool(p) => Ok(p.into_py(py)),
        Value::Number(n) => {
            if let Some(k) = n.as_i64() {
                Ok(k.into_py(py))
            } else if let Some(x) = n.as_f64() {
                Ok(x.into_py(py))
            } else {
                Err(Error::from(ErrorKind::InvalidNumber(n.clone())))
            }
        }
        Value::String(s) => Ok(s.into_py(py)),
    }
}

pub fn json_match<'v>(
    json_path: Option<&JsonPath>,
    payload: &'v serde_json::Value,
) -> Option<serde_json::Value> {
    if let Some(path) = json_path {
        let values = path
            .query(payload)
            .all()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        match values.len() {
            0 => None,
            1 => Some(values.into_iter().next().unwrap()),
            _ => Some(serde_json::Value::Array(values)),
        }
    } else {
        Some(payload.clone())
    }
}

pub fn to_polars<T: Serialize>(values: &[T], config: Option<TracingOptions>) -> Result<DataFrame> {
    let config = config.unwrap_or_else(TracingOptions::default);
    let fields = serialize_into_fields(values, config)?;
    let array = serialize_into_arrays(fields.as_slice(), values)?;
    let chunk = Chunk::try_new(array)?;
    Ok(DataFrame::try_from((chunk, fields.as_ref()))?)
}
