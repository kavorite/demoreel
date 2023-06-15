This is a [PyO3] module that exposes .dem packet traces (demo-files) generated
by Source games as Python `dict`s and rolls wheels using [maturin], motivated by
the need for integration into [MAC/Jensen][jensen]. 

Using this library, data storage and annotation can be decoupled from the batch
processing of the demo-files, avoiding the introduction of a domain-specific
format for training of AI models, and hopefully helping separate the concerns of
individual applications consuming such data from the pipelines producing and
ingesting it. 

Currently filters out everything except user input, following operational
principles of https://donadigo.com/tmx1. This is subject to change, but
intuitively, if a player performs 3 consecutive frame-perfect b-hops or snaps of
their barrel between the player position closest to their current view angle and
some other random direction in a small cone, then you probably don't actually
need to know how much damage they did in order to identify that they're hacking.


- [ ] Flatten + yield higher-level types of messages
    - [ ] Specify allowed types from Python land (maybe as [JSON paths][jpath]?)

[maturin]: https://maturin.rs/
[pyo3]: https://pyo3.rs/
[jpath]: https://docs.rs/serde_json_path/
[jensen]: https://github.com/megascatterbomb/MegaAntiCheat/