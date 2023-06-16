#[macro_use]
extern crate error_chain;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use serde::Serialize;
use tf_demo_parser::demo::packet::Packet;
use tf_demo_parser::demo::parser::gamestateanalyser::{GameState, GameStateAnalyser};
use tf_demo_parser::demo::parser::DemoParser;
use tf_demo_parser::Demo;

use pyo3::types::{PyDict, PyList};
use serde_json_path::JsonPath;

#[derive(Serialize)]
struct Traced<'t>(Packet<'t>);
mod errors {
    error_chain! {
        errors {
            InvalidNumber(x: serde_json::Number) {
                description("invalid number"),
                display("'{}' cannot be represented either as i- or f-64", x)
            }
        }

        foreign_links {
            WireFormat(tf_demo_parser::ParseError);
            Buffering(bitbuffer::BitError);
            Python(pyo3::PyErr);
            Io(std::io::Error);
            Json(serde_json::Error);
            PathParse(serde_json_path::ParseError);
            PathMatch(serde_json_path::AtMostOneError);
        }
    }
}

use errors::*;

impl std::convert::From<Error> for PyErr {
    fn from(err: Error) -> PyErr {
        PyValueError::new_err(err.to_string())
    }
}

fn json_to_py<'py>(py: Python<'py>, v: &serde_json::Value) -> Result<PyObject> {
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

fn json_match<'v>(
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

#[derive(Serialize)]
struct Snapshot<'s>(&'s GameState);

/// parse(buf, /)
/// --
///
/// Parses bytes into raw player inputs.
#[pyfunction]
#[pyo3(signature = (buf, json_path=None, tick_freq=1))]
fn unspool<'py>(
    py: Python<'py>,
    buf: &'_ [u8],
    json_path: Option<&'_ str>,
    tick_freq: Option<u32>,
) -> Result<&'py PyList> {
    let matches = py.allow_threads(|| -> Result<_> {
        let path: Option<JsonPath> = json_path.map(JsonPath::parse).transpose()?;
        let mut matches = Vec::new();
        let demo = Demo::new(&buf);
        let stream = demo.get_stream();
        let parser = DemoParser::new_with_analyser(stream, GameStateAnalyser::new());
        let (_header, mut ticker) = parser.ticker()?;
        while let Some(t) = ticker.next()? {
            let tick: u32 = t.tick.into();
            if (tick % tick_freq.unwrap_or(1)) == 0 {
                let value = serde_json::to_value(t.state)?;
                if let Some(v) = json_match(path.as_ref(), &value) {
                    matches.push(v);
                }
            }
        }
        Ok(matches)
    })?;
    let objects = matches
        .into_iter()
        .map(|v| json_to_py(py, &v))
        .collect::<Result<Vec<_>>>()?;
    Ok(PyList::new(py, objects))
}

#[pymodule]
fn demoreel(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(unspool, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const PAYLOAD: &'static [u8] = include_bytes!("../Round_1_Map_1_Borneo.dem");
    #[test]
    fn unspool_succeeds() {
        Python::with_gil(|py| {
            assert!(unspool(py, PAYLOAD, None, None).is_ok());
        });
        todo!("better tests pls");
    }
}
