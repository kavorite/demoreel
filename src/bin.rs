use clap::Parser;

use demoreel::errors::Result;
use demoreel::serialize::json_match;
use demoreel::tracer::PacketStream;
use serde::Serialize;
use serde_json_path::JsonPath;
use tf_demo_parser::Demo;

#[derive(Parser)]
struct Args {
    #[arg(short, long)]
    json_path: Option<String>,
    file_name: String,
}

fn pipe(
    ostrm: &mut std::io::StdoutLock,
    jpath: Option<&JsonPath>,
    value: &impl Serialize,
) -> Result<()> {
    if let Some(value) = {
        let value = serde_json::to_value(value)?;
        json_match(jpath, &value)
    } {
        serde_json::to_writer(ostrm, &value)?;
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let jpath = args
        .json_path
        .as_ref()
        .map(String::as_str)
        .map(JsonPath::parse)
        .transpose()?;
    let data = std::fs::read(args.file_name)?;
    let mut ostrm = {
        let stdout = std::io::stdout();
        stdout.lock()
    };
    let istrm = PacketStream::new(Demo::new(data.as_slice()))?;
    pipe(&mut ostrm, jpath.as_ref(), istrm.header())?;
    for result in istrm {
        let packet = result?;
        pipe(&mut ostrm, jpath.as_ref(), &packet)?;
    }
    Ok(())
}
