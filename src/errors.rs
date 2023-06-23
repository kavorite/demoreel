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
        ArrowSerialization(serde_arrow::Error);
        Arrow(arrow2::error::Error);
        PathParse(serde_json_path::ParseError);
        PathMatch(serde_json_path::AtMostOneError);
        Polars(polars::error::PolarsError);
    }
}

use pyo3::{exceptions::PyValueError, PyErr};

impl std::convert::From<Error> for PyErr {
    fn from(err: Error) -> PyErr {
        PyValueError::new_err(err.to_string())
    }
}
