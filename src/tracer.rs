use crate::errors::{Error, Result};

use crate::serialize::to_polars;
use bitbuffer::BitRead;
use itertools::Itertools;
use polars::prelude::*;
use serde::Serialize;
use serde_arrow::schema::TracingOptions;
use tf_demo_parser::demo::data::userinfo::PlayerInfo;
use tf_demo_parser::demo::data::userinfo::UserInfo;
use tf_demo_parser::demo::gameevent_gen::PlayerHurtEvent;
use tf_demo_parser::demo::gamevent::GameEvent;
use tf_demo_parser::demo::header::Header;
use tf_demo_parser::demo::message::gameevent::GameEventMessage;
use tf_demo_parser::demo::message::Message;
use tf_demo_parser::demo::packet::Packet;
use tf_demo_parser::demo::parser::gamestateanalyser::{
    Class, GameStateAnalyser, Player, PlayerState, Team, UserId, World,
};
use tf_demo_parser::demo::parser::handler::BorrowMessageHandler;
use tf_demo_parser::demo::parser::{DemoHandler, MessageHandler, NullHandler, RawPacketStream};
use tf_demo_parser::demo::vector::Vector;
use tf_demo_parser::{Demo, MessageType};

pub struct PacketStream<'s, 'h> {
    packets: RawPacketStream<'s>,
    handler: DemoHandler<'h, NullHandler>,
    header: Header,
}

impl<'s, 'h> PacketStream<'s, 'h> {
    pub fn new(demo: Demo<'s>) -> Result<Self> {
        let mut stream = demo.get_stream();
        let mut handler = DemoHandler::default();
        let header = Header::read(&mut stream)?;
        handler.handle_header(&header);
        let packets = RawPacketStream::new(stream);
        Ok(Self {
            header,
            handler,
            packets,
        })
    }

    pub fn header(&self) -> &Header {
        &self.header
    }
}

impl<'s, 'h> Iterator for PacketStream<'s, 'h> {
    type Item = Result<Packet<'s>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.packets
            .next(&self.handler.state_handler)
            .map_err(Error::from)
            .transpose()
    }
}

#[derive(Serialize, Clone)]
pub struct Profile {
    pub name: String,
    pub user_id: UserId,
    pub steam_id: String,
    pub friends_id: u32,
    pub is_fake_player: bool,
    pub is_hl_tv: bool,
    pub is_replay: bool,
    pub custom_file: [u32; 4],
    pub files_downloaded: u32,
    pub more_extra: bool,
}

impl From<PlayerInfo> for Profile {
    fn from(player: PlayerInfo) -> Self {
        Self {
            friends_id: player.friends_id,
            user_id: player.user_id,
            name: player.name,
            steam_id: player.steam_id,
            is_fake_player: player.is_fake_player != 0,
            is_hl_tv: player.is_hl_tv != 0,
            is_replay: player.is_replay != 0,
            custom_file: player.custom_file,
            files_downloaded: player.files_downloaded,
            more_extra: player.more_extra != 0,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct Snapshot {
    pub position: Vector,
    pub health: u16,
    pub max_health: u16,
    pub class: &'static str,
    pub team: &'static str,
    pub view_angle: f32,
    pub pitch_angle: f32,
    pub state: &'static str,
    pub user_id: Option<u16>,
    pub charge: u8,
    pub in_pvs: bool,
    pub simtime: u16,
}

impl From<Player> for Snapshot {
    fn from(value: Player) -> Self {
        Self {
            position: value.position,
            health: value.health,
            max_health: value.max_health,
            class: match value.class {
                Class::Scout => "scout",
                Class::Soldier => "soldier",
                Class::Pyro => "pyro",
                Class::Demoman => "demoman",
                Class::Heavy => "heavy",
                Class::Engineer => "engineer",
                Class::Medic => "medic",
                Class::Sniper => "sniper",
                Class::Spy => "spy",
                Class::Other => "other",
            },
            team: match value.team {
                Team::Blue => "blu",
                Team::Red => "red",
                Team::Spectator => "spectator",
                Team::Other => "other",
            },
            view_angle: value.view_angle,
            pitch_angle: value.pitch_angle,
            state: match value.state {
                PlayerState::Alive => "alive",
                PlayerState::Death => "death",
                PlayerState::Dying => "dying",
                PlayerState::Respawnable => "queue",
            },
            user_id: value.info.map(|info| info.user_id.into()),
            charge: value.charge,
            simtime: value.simtime,
            in_pvs: value.in_pvs,
        }
    }
}

#[derive(Serialize, Clone)]
pub struct WithTick<T: Serialize + Clone> {
    pub inner: T,
    pub tick: u32,
}

impl<T: Serialize + Clone> WithTick<T> {
    pub fn to_polars(
        items: impl Iterator<Item = WithTick<T>>,
        tropt: Option<TracingOptions>,
    ) -> Result<DataFrame> {
        let (ticks, inner): (Vec<u32>, Vec<T>) =
            items.map(|WithTick { tick, inner }| (tick, inner)).unzip();
        let ticks = Series::new("tick", ticks);
        let mut frame = to_polars(inner.as_slice(), tropt)?;
        let frame = std::mem::take(frame.with_column(ticks)?);
        Ok(frame)
    }
}

pub struct Roster {
    pub roster: Vec<Profile>,
    user_ids: Vec<UserId>,
}

impl Roster {
    pub fn new() -> Self {
        let roster = Vec::new();
        let user_ids = Vec::new();
        Self { roster, user_ids }
    }
}

impl MessageHandler for Roster {
    type Output = Self;

    fn does_handle(message_type: MessageType) -> bool {
        matches!(
            message_type,
            MessageType::UpdateStringTable | MessageType::CreateStringTable
        )
    }

    fn handle_string_entry(
        &mut self,
        table: &str,
        index: usize,
        entry: &tf_demo_parser::demo::packet::stringtable::StringTableEntry,
        _parser_state: &tf_demo_parser::ParserState,
    ) {
        if table == "userinfo" {
            if let Some(UserInfo {
                player_info: player,
                ..
            }) = {
                let index = index as u16;
                let text = entry.text.as_ref().map(AsRef::as_ref);
                let data = entry.extra_data.as_ref().map(|extra| extra.data.clone());
                UserInfo::parse_from_string_table(index, text, data).unwrap()
            } {
                if !self.user_ids.contains(&player.user_id) {
                    self.user_ids.push(player.user_id);
                    self.roster.push(Profile::from(player));
                }
            }
        }
    }

    fn into_output(self, _state: &tf_demo_parser::ParserState) -> Self {
        self
    }
}

pub struct Tracer {
    pub integrator: GameStateAnalyser,
    pub events: Vec<WithTick<PlayerHurtEvent>>,
    pub states: Vec<WithTick<Snapshot>>,
    pub roster: Roster,
    pub bounds: Vec<WithTick<World>>,
    deltas: Vec<Player>,
}

impl Tracer {
    pub fn new() -> Self {
        Self {
            integrator: GameStateAnalyser::new(),
            states: Vec::new(),
            deltas: Vec::new(),
            events: Vec::new(),
            roster: Roster::new(),
            bounds: Vec::new(),
        }
    }

    fn compute_deltas(
        &mut self,
        message: &tf_demo_parser::demo::message::Message,
        tick: tf_demo_parser::demo::data::DemoTick,
        parser_state: &tf_demo_parser::ParserState,
    ) {
        let prev_states = self.integrator.state.players.clone();
        self.integrator.handle_message(message, tick, parser_state);
        self.deltas = self
            .integrator
            .state
            .players
            .iter()
            .chain(prev_states.iter())
            .unique_by(|player| player.info.as_ref().map(|info| info.user_id))
            .cloned()
            .collect();
    }
}

impl MessageHandler for Tracer {
    type Output = Self;

    fn does_handle(message_type: MessageType) -> bool {
        matches!(
            message_type,
            MessageType::GameEvent
                | MessageType::CreateStringTable
                | MessageType::UpdateStringTable
        ) || GameStateAnalyser::does_handle(message_type)
    }

    fn handle_header(&mut self, header: &tf_demo_parser::demo::header::Header) {
        self.integrator.handle_header(header);
    }

    fn handle_message(
        &mut self,
        message: &tf_demo_parser::demo::message::Message,
        tick: tf_demo_parser::demo::data::DemoTick,
        parser_state: &tf_demo_parser::ParserState,
    ) {
        self.integrator.handle_message(message, tick, parser_state);
        let bounds = match (self.bounds.last(), self.integrator.state.world.as_ref()) {
            (None, Some(bounds)) => Some(bounds),
            (Some(prev), Some(next)) if &prev.inner != next => Some(next),
            _ => None,
        };
        if let Some(bounds) = bounds {
            let inner = bounds.clone();
            let tick = tick.into();
            self.bounds.push(WithTick { inner, tick });
        }
        if let Message::GameEvent(GameEventMessage {
            event: GameEvent::PlayerHurt(event),
            ..
        }) = message
        {
            let inner = event.clone();
            let tick = tick.into();
            self.events.push(WithTick { tick, inner });
        }
        self.compute_deltas(message, tick, parser_state);
        for player in std::mem::take(&mut self.deltas).into_iter() {
            if player.info.is_some() {
                let inner = player.clone().into();
                let tick = tick.into();
                self.states.push(WithTick { tick, inner });
            }
        }
    }

    fn handle_string_entry(
        &mut self,
        table: &str,
        index: usize,
        entry: &tf_demo_parser::demo::packet::stringtable::StringTableEntry,
        parser_state: &tf_demo_parser::ParserState,
    ) {
        self.integrator
            .handle_string_entry(table, index, entry, parser_state);
        self.roster
            .handle_string_entry(table, index, entry, parser_state);
    }

    fn handle_data_tables(
        &mut self,
        tables: &[tf_demo_parser::demo::packet::datatable::ParseSendTable],
        server_classes: &[tf_demo_parser::demo::packet::datatable::ServerClass],
        parser_state: &tf_demo_parser::ParserState,
    ) {
        self.integrator
            .handle_data_tables(tables, server_classes, parser_state);
    }

    fn handle_packet_meta(
        &mut self,
        tick: tf_demo_parser::demo::data::DemoTick,
        meta: &tf_demo_parser::demo::packet::message::MessagePacketMeta,
        parser_state: &tf_demo_parser::ParserState,
    ) {
        self.integrator.handle_packet_meta(tick, meta, parser_state);
    }

    fn into_output(self, _state: &tf_demo_parser::ParserState) -> Self::Output {
        self
    }
}

impl BorrowMessageHandler for Tracer {
    fn borrow_output(&self, _state: &tf_demo_parser::ParserState) -> &Self::Output {
        &self
    }
}
