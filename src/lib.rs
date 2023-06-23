#[macro_use]
extern crate error_chain;

mod errors;
mod serialize;
mod tracer;

use polars::prelude::*;
use pyo3::prelude::*;
use pyo3_polars::PyDataFrame;
use serde::Serialize;
use serde_arrow::schema::TracingOptions;
use tf_demo_parser::demo::parser::gamestateanalyser::{GameState, GameStateAnalyser};
use tf_demo_parser::demo::parser::DemoParser;
use tf_demo_parser::Demo;
use tracer::{DamageTracer, RosterAnalyser, Snapshot, TickSnapshot};

use errors::*;
use pyo3::types::PyList;
use serde_json_path::JsonPath;
use serialize::{json_match, json_to_py, to_polars};

#[cfg(test)]
mod tests {
    use super::*;
    const PAYLOAD: &'static [u8] = include_bytes!("../demos/Round_1_Map_1_Borneo.dem");
    #[test]
    fn dtrace_succeeds() {
        Python::with_gil(|py| {
            // assert!(unspool(py, PAYLOAD, None, None).is_ok());
            // let target = Some("[U:1:82537314]".to_owned());
            let target = None;
            assert!(dtrace(py, PAYLOAD, target).is_ok());
            // assert!(roster(py, PAYLOAD).is_ok());
        });
    }
}

/// Return metadata identifying all players present in the replay.
#[pyfunction]
#[pyo3(signature = (buf))]
fn roster<'py>(py: Python<'py>, buf: &[u8]) -> Result<PyDataFrame> {
    py.allow_threads(|| -> Result<_> {
        let demo = Demo::new(&buf);
        let stream = demo.get_stream();
        let parser = DemoParser::new_with_analyser(stream, RosterAnalyser::new());
        let (_header, roster) = parser.parse()?;
        Ok(PyDataFrame(to_polars(roster.players.as_ref(), None)?))
    })
}

#[pyfunction]
#[pyo3(signature = (buf))]
fn bounds<'py>(py: Python<'py>, buf: &[u8]) -> Result<PyDataFrame> {
    let worlds = py.allow_threads(|| -> Result<_> {
        let demo = Demo::new(&buf);
        let stream = demo.get_stream();
        let parser = DemoParser::new_with_analyser(stream, GameStateAnalyser::new());
        let (_header, mut ticker) = parser.ticker()?;
        let mut prev_world = None;
        let mut worlds = Vec::new();
        let mut ticks = Vec::new();
        while let Some(t) = ticker.next()? {
            if prev_world.as_ref() != t.state.world.as_ref() {
                prev_world = t.state.world.clone();
                ticks.push(u32::from(t.tick));
                if let Some(world) = &t.state.world {
                    worlds.push(world.clone());
                }
            }
        }
        let mut frame = to_polars(worlds.as_slice(), None)?;
        let mut ticks = Series::new("tick", ticks);
        ticks.set_sorted_flag(polars::series::IsSorted::Ascending);
        let frame = frame.with_column(ticks)?;
        Ok(std::mem::take(frame))
    })?;
    Ok(PyDataFrame(worlds))
}

/// Trace each instance of damage back over the states of the players having
/// dealt them, interleaving the state of the source and the target. If a
/// particular player is specified as a source, buffer their status and their
/// victims' only.
#[pyfunction]
#[pyo3(signature = (buf, source=None))]
fn dtrace<'py>(py: Python<'py>, buf: &[u8], source: Option<String>) -> Result<PyObject> /* Result<PyDataFrame> */
{
    let states = py.allow_threads(|| -> Result<_> {
        let demo = Demo::new(&buf);
        let stream = demo.get_stream();
        let tracer = DamageTracer::new(source);
        let parser = DemoParser::new_with_analyser(stream, tracer);
        let (_header, mut ticker) = parser.ticker()?;
        let mut states = DataFrame::empty();
        states.align_chunks();
        let mut prev_uids = None;
        while let Some(t) = ticker.next()? {
            if let Some(state) = t.state.borrow_mut().take() {
                let uids = state.source.user_id.zip(state.victim.user_id);
                if uids != prev_uids
                    && uids.map(|(v, t)| v != t).unwrap_or(false)
                    && !state.states.is_empty()
                {
                    prev_uids = prev_uids.take().or(uids);
                    let (ticks, snaps): (Vec<u32>, Vec<Snapshot>) = state
                        .states
                        .into_iter()
                        .map(|TickSnapshot { tick, snapshot }| (tick, snapshot))
                        .unzip();
                    // TODO: why don't victim/attacker states match the underlying IDs?
                    let (_, victim_id) = uids.unzip();
                    let mut is_victim: Series =
                        snaps.iter().map(|u| u.user_id == victim_id).collect();
                    is_victim = std::mem::take(is_victim.rename("is_victim"));
                    let tropt = TracingOptions::default()
                        .allow_null_fields(true)
                        .string_dictionary_encoding(false); // TOOD: figure out why we can't do this
                    let ticks = Series::new("tick", ticks);
                    let mut frame = to_polars(snaps.as_slice(), Some(tropt))?;
                    let frame = frame.with_column(ticks)?;
                    let frame = frame.with_column(is_victim)?;
                    states.vstack_mut(&frame)?;
                }
            }
        }
        Ok(PyDataFrame(states))
    })?;
    let output = states.into_py(py);
    Ok(output)
}

/// Parses the .dem wire format into a JSON representation of player states.
#[pyfunction]
#[pyo3(signature = (buf, json_path=None, tick_freq=1))]
fn unspool<'py>(
    py: Python<'py>,
    buf: &[u8],
    json_path: Option<&str>,
    tick_freq: Option<u32>,
) -> Result<&'py PyList> {
    #[derive(Serialize)]
    struct Snapshot<'s>(&'s GameState);

    let matches = py.allow_threads(|| -> Result<_> {
        let path: Option<JsonPath> = json_path.map(JsonPath::parse).transpose()?;
        let mut matches = Vec::new();
        let demo = Demo::new(&buf);
        let stream = demo.get_stream();
        let parser = DemoParser::new_with_analyser(stream, GameStateAnalyser::new());
        let (_header, mut ticker) = parser.ticker()?;
        let mut tick_seq: u32 = 0;
        let mut prev_tick = None;
        while let Some(t) = ticker.next()? {
            if let Some(prev) = prev_tick {
                if prev != t.tick {
                    tick_seq = tick_seq.wrapping_add(1);
                }
                if tick_seq % tick_freq.unwrap_or(1) != 0 {
                    continue;
                }
            }
            prev_tick = Some(t.tick);
            if (tick_seq % tick_freq.unwrap_or(1)) == 0 {
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
    m.add_function(wrap_pyfunction!(dtrace, m)?)?;
    m.add_function(wrap_pyfunction!(unspool, m)?)?;
    m.add_function(wrap_pyfunction!(roster, m)?)?;
    m.add_function(wrap_pyfunction!(bounds, m)?)?;
    Ok(())
}
