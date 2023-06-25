use bitbuffer::BitRead;
use demoreel::errors::Result;
use demoreel::serialize::json_match;
use serde_json_path::JsonPath;
use std::io::Read;
use tf_demo_parser::demo::header::Header;
use tf_demo_parser::demo::parser::*;
use tf_demo_parser::Demo;

fn main() -> Result<()> {
    let jpath = std::env::args()
        .skip(1)
        .next()
        .as_ref()
        .map(String::as_str)
        .map(JsonPath::parse)
        .transpose()?;
    let data = {
        let stdin = std::io::stdin();
        let mut istrm = stdin.lock();
        let mut data = Vec::new();
        istrm.read_to_end(&mut data)?;
        data
    };
    let mut ostrm = {
        let stdout = std::io::stdout();
        stdout.lock()
    };
    let demo = Demo::new(data.as_slice());
    let mut handler = DemoHandler::default();
    let mut stream = demo.get_stream();
    let header = Header::read(&mut stream)?;
    handler.handle_header(&header);
    let value = {
        let value = serde_json::to_value(&header)?;
        json_match(jpath.as_ref(), &value)
    };
    if let Some(value) = value {
        serde_json::to_writer(&mut ostrm, &value)?;
    }
    let mut packets = RawPacketStream::new(stream);

    while let Some(packet) = packets.next(&handler.state_handler)? {
        let value = {
            let value = serde_json::to_value(&packet)?;
            json_match(jpath.as_ref(), &value)
        };
        if let Some(value) = value {
            serde_json::to_writer(&mut ostrm, &value)?;
        }
        handler.handle_packet(packet)?;
    }
    Ok(())
}
