#[macro_use]
extern crate error_chain;

use bitbuffer::BitRead;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use serde::Serialize;
use tf_demo_parser::demo::header::Header;
use tf_demo_parser::demo::packet::Packet;
use tf_demo_parser::demo::parser::{DemoHandler, RawPacketStream};
use tf_demo_parser::Demo;

use pyo3::types::{PyBool, PyDict, PyList, PyString};

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
            Parsing(tf_demo_parser::ParseError);
            Buffering(bitbuffer::BitError);
            Python(pyo3::PyErr);
            Io(std::io::Error);
            Json(serde_json::Error);
        }
    }
}

use errors::*;

impl std::convert::From<Error> for PyErr {
    fn from(err: Error) -> PyErr {
        PyValueError::new_err(err.to_string())
    }
}

fn json_to_py<'py>(py: Python<'py>, v: serde_json::Value) -> Result<PyObject> {
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
        Value::Bool(p) => Ok(PyBool::new(py, p).into()),
        Value::Number(n) => {
            if let Some(k) = n.as_i64() {
                Ok(k.into_py(py))
            } else if let Some(x) = n.as_f64() {
                Ok(x.into_py(py))
            } else {
                Err(Error::from(ErrorKind::InvalidNumber(n)))
            }
        }
        Value::String(s) => Ok(PyString::new(py, s.as_ref()).into()),
    }
}

/// parse(buf, /)
/// --
///
/// Parses bytes into raw player inputs.
#[pyfunction]
#[pyo3(signature = (buf))]
fn unspool<'py>(py: Python<'py>, buf: &'py [u8]) -> Result<&'py PyList> {
    let cmds = py.allow_threads(|| -> Result<_> {
        let mut elements = Vec::new();
        let demo = Demo::new(&buf);
        let mut handler = DemoHandler::default();

        let mut stream = demo.get_stream();
        let _header = Header::read(&mut stream).chain_err(|| "Invalid header")?;

        let mut packets = RawPacketStream::new(stream);

        while let Some(packet) = packets.next(&handler.state_handler)? {
            if let Packet::UserCmd(ref cmd_packet) = packet {
                let payload = serde_json::to_value(cmd_packet)
                    .chain_err(|| "Packet cannot be mapped to JSON object")?;
                elements.push(payload);
            }
            handler
                .handle_packet(packet)
                .chain_err(|| "Invalid stream")?;
        }
        Ok(elements)
    })?;
    let r: Result<Vec<_>> = cmds.into_iter().map(|p| json_to_py(py, p)).collect();
    Ok(PyList::new(py, r?))
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
            assert!(unspool(py, PAYLOAD).is_ok());
        });
        todo!("better tests pls");
    }
}
