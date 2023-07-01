#[macro_use]
extern crate error_chain;

pub mod errors;
pub mod serialize;
pub mod tracer;

use pyo3::prelude::*;
use pyo3_polars::PyDataFrame;
use serde_arrow::schema::TracingOptions;
use tf_demo_parser::demo::parser::DemoParser;
use tf_demo_parser::Demo;
use tracer::{Roster, Tracer, WithTick};

use errors::*;
use serialize::to_polars;

#[cfg(test)]
mod tests {
    use crate::tracer::PacketStream;

    use super::*;
    const BORNEO: &'static [u8] = include_bytes!("../demos/Round_1_Map_1_Borneo.dem");
    const FLAG_UPDATES: &'static [u8] = include_bytes!("../demos/flag_updates.dem");
    #[test]
    fn dtrace_succeeds() {
        Python::with_gil(|py| {
            assert!(dtrace(py, BORNEO).is_ok());
            // assert!(roster(py, PAYLOAD).is_ok());
        });
    }

    #[test]
    fn log_flag_updates() {
        let demo = Demo::new(FLAG_UPDATES);
        let packets = PacketStream::new(demo).unwrap();

        for result in packets {
            let packet = result.unwrap();
            match &packet {
                tf_demo_parser::demo::packet::Packet::ConsoleCmd(cmd) => {
                    if cmd.command.starts_with("echo ") {
                        println!("{}", cmd.command);
                    }
                }
                _ => {}
            }
        }
    }
}

#[pyclass(get_all)]
pub struct DTrace {
    states: PyDataFrame,
    events: PyDataFrame,
    roster: PyDataFrame,
    bounds: PyDataFrame,
}

#[pyfunction]
fn roster<'py>(py: Python<'py>, buffer: &[u8]) -> Result<PyDataFrame> {
    py.allow_threads(|| -> Result<_> {
        let demo = Demo::new(&buffer);
        let stream = demo.get_stream();
        let parser = DemoParser::new_with_analyser(stream, Roster::new());
        let (_header, roster) = parser.parse()?;
        Ok(PyDataFrame(to_polars(roster.roster.as_slice(), None)?))
    })
}

/// Trace all all players, states, and instances of damage inflicted within a
/// demo file, yielding the result as a set of polars dataframes.
#[pyfunction]
#[pyo3(signature = (buffer))]
fn dtrace<'py>(py: Python<'py>, buffer: &[u8]) -> Result<DTrace> {
    let (states, events, roster, bounds) = py.allow_threads(|| -> Result<_> {
        let demo = Demo::new(&buffer);
        let stream = demo.get_stream();
        let parser = DemoParser::new_with_analyser(stream, Tracer::new());
        let (_header, dtrace) = parser.parse()?;
        let tropt = TracingOptions::default()
            .allow_null_fields(true)
            .string_dictionary_encoding(false);
        let states = WithTick::to_polars(dtrace.states.into_iter(), Some(tropt.clone()))?;
        let events = WithTick::to_polars(dtrace.events.into_iter(), Some(tropt.clone()))?;
        let bounds = WithTick::to_polars(dtrace.bounds.into_iter(), Some(tropt.clone()))?;
        let roster = to_polars(dtrace.roster.roster.as_slice(), Some(tropt.clone()))?;
        Ok((
            PyDataFrame(states),
            PyDataFrame(events),
            PyDataFrame(roster),
            PyDataFrame(bounds),
        ))
    })?;
    let dtrace = DTrace {
        states,
        events,
        roster,
        bounds,
    };
    Ok(dtrace)
}

#[pymodule]
fn demoreel(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(dtrace, m)?)?;
    m.add_function(wrap_pyfunction!(roster, m)?)?;
    Ok(())
}
