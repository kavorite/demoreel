use std::cell::RefCell;
use std::collections::BTreeMap;

use itertools::Itertools;
use serde::Serialize;
use tf_demo_parser::demo::gamevent::GameEvent;
use tf_demo_parser::demo::message::gameevent::GameEventMessage;
use tf_demo_parser::demo::message::Message;
use tf_demo_parser::demo::parser::analyser::UserInfo;
use tf_demo_parser::demo::parser::gamestateanalyser::{
    Class, GameStateAnalyser, Player, PlayerState, Team, UserId,
};
use tf_demo_parser::demo::parser::handler::BorrowMessageHandler;
use tf_demo_parser::demo::parser::MessageHandler;
use tf_demo_parser::demo::vector::Vector;
use tf_demo_parser::MessageType;

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

#[derive(Clone, Serialize)]
pub struct RosterAnalyser {
    pub players: Vec<Profile>,
    #[serde(skip_serializing)]
    player_ids: Vec<UserId>,
}

impl RosterAnalyser {
    pub fn new() -> Self {
        Self {
            players: Vec::new(),
            player_ids: Vec::new(),
        }
    }
}

impl MessageHandler for RosterAnalyser {
    type Output = Self;

    fn handle_string_entry(
        &mut self,
        table: &str,
        index: usize,
        entry: &tf_demo_parser::demo::packet::stringtable::StringTableEntry,
        _parser_state: &tf_demo_parser::ParserState,
    ) {
        use tf_demo_parser::demo::data::userinfo::UserInfo;
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
                if !self.player_ids.contains(&player.user_id) {
                    self.players.push(Profile {
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
                    });
                    self.player_ids.push(player.user_id);
                }
            }
        }
    }

    fn does_handle(message_type: tf_demo_parser::MessageType) -> bool {
        matches!(
            message_type,
            MessageType::CreateStringTable | MessageType::UpdateStringTable
        )
    }

    fn into_output(self, _state: &tf_demo_parser::ParserState) -> Self::Output {
        self
    }
}

#[derive(Serialize, Clone)]
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
        }
    }
}

#[derive(Serialize, Clone)]
pub struct TickSnapshot {
    pub snapshot: Snapshot,
    pub tick: u32,
}

#[derive(Clone, Serialize)]
pub struct DamageTrace {
    #[serde(skip_serializing)]
    pub source: Snapshot,
    #[serde(skip_serializing)]
    pub victim: Snapshot,
    pub states: Vec<TickSnapshot>,
}

pub struct DamageTracer {
    pub source: Option<Player>,
    pub source_guid: Option<String>,
    pub integrator: GameStateAnalyser,
    pub traced: RefCell<Option<DamageTrace>>,
    pub traces: BTreeMap<u16, Vec<TickSnapshot>>,
    deltas: Vec<Player>,
}
impl DamageTracer {
    pub fn new(source_guid: Option<String>) -> Self {
        Self {
            source_guid,
            source: None,
            integrator: GameStateAnalyser::new(),
            traced: None.into(),
            traces: BTreeMap::new(),
            deltas: Vec::new(),
        }
    }

    fn compute_deltas(
        &mut self,
        message: &tf_demo_parser::demo::message::Message,
        tick: tf_demo_parser::demo::data::DemoTick,
        parser_state: &tf_demo_parser::ParserState,
    ) {
        self.deltas = {
            let prev_states = self.integrator.state.players.clone();
            self.integrator.handle_message(message, tick, parser_state);
            self.integrator
                .state
                .players
                .iter()
                .chain(prev_states.iter())
                .unique_by(|player| player.info.as_ref().map(|info| info.user_id))
                .cloned()
                .collect()
        };
    }

    fn player_by_guid(&self, target: &str) -> Option<&Player> {
        for player in self.integrator.state.players.iter() {
            if let Some(UserInfo { steam_id, .. }) = &player.info {
                if target == steam_id {
                    return Some(player);
                }
            }
        }
        None
    }

    fn player_by_id(&self, target: u16) -> Option<&Player> {
        for player in self.integrator.state.players.iter() {
            if let Some(UserInfo { user_id, .. }) = player.info {
                if target == Into::<u16>::into(user_id) {
                    return Some(player);
                }
            }
        }
        None
    }
}

impl MessageHandler for DamageTracer {
    type Output = RefCell<Option<DamageTrace>>;

    fn does_handle(message_type: MessageType) -> bool {
        matches!(message_type, MessageType::GameEvent)
            | GameStateAnalyser::does_handle(message_type)
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
        self.compute_deltas(message, tick, parser_state);
        for player in std::mem::take(&mut self.deltas).into_iter() {
            if let Some(UserInfo { user_id, .. }) = player.info {
                let buffer = self.traces.entry(user_id.into()).or_insert(Vec::new());
                let snapshot = player.clone().into();
                let tick = tick.into();
                buffer.push(TickSnapshot { tick, snapshot });
            }
        }
        match message {
            Message::GameEvent(GameEventMessage {
                event: GameEvent::PlayerHurt(event),
                ..
            }) => {
                let source = if let Some(guid) = &self.source_guid {
                    self.source
                        .take()
                        .or_else(|| self.player_by_guid(&guid).cloned())
                } else {
                    self.player_by_id(event.attacker).cloned()
                };
                if let Some(source) = source {
                    let target = self.player_by_id(event.user_id);
                    let (prev_victim_is_target, some_source_is_attker) = {
                        let traced = self.traced.borrow();
                        let prev_victim = traced.as_ref().map(|traced| &traced.victim);
                        // TODO: surface weapon types, crit status
                        let victim_target = prev_victim
                            .and_then(|u| u.user_id)
                            .zip(target.and_then(|u| u.info.as_ref()))
                            .map(|(v, t)| v == u16::from(t.user_id))
                            .unwrap_or(false);
                        let source_attker = source
                            .info
                            .as_ref()
                            .map(|info| info.user_id == event.attacker)
                            .unwrap_or(true);
                        (victim_target, source_attker)
                    };
                    if let Some(victim) = self.player_by_id(event.user_id).cloned() {
                        if some_source_is_attker
                            && !prev_victim_is_target
                            && victim.info.is_some()
                            && self
                                .traces
                                .get(&u16::from(victim.info.as_ref().unwrap().user_id))
                                .map(|trace| trace.len())
                                .unwrap_or(0)
                                > 0
                        {
                            let states = self
                                .traces
                                .remove(&event.user_id)
                                .into_iter()
                                .flatten()
                                .rev()
                                .take(128)
                                .interleave(
                                    self.traces
                                        .remove(&event.attacker)
                                        .into_iter()
                                        .flatten()
                                        .rev()
                                        .take(128),
                                )
                                .collect::<Vec<_>>();
                            let traced = DamageTrace {
                                states,
                                source: source.clone().into(),
                                victim: victim.into(),
                            };
                            self.traced = RefCell::new(Some(traced));
                        }
                    }
                    self.source = Some(source);
                }
            }
            _ => {
                // Message::PacketEntities(packet) => {
                // retrieve and trace states for all players that received an update
                // let ent_ids: Vec<_> = packet.entities.iter().map(|ent| ent.entity_index).collect();
            }
        };
    }

    fn handle_string_entry(
        &mut self,
        table: &str,
        index: usize,
        entries: &tf_demo_parser::demo::packet::stringtable::StringTableEntry,
        parser_state: &tf_demo_parser::ParserState,
    ) {
        self.integrator
            .handle_string_entry(table, index, entries, parser_state);
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
        self.traced
    }
}

impl BorrowMessageHandler for DamageTracer {
    fn borrow_output(&self, _state: &tf_demo_parser::ParserState) -> &Self::Output {
        &self.traced
    }
}
