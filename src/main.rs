#![recursion_limit = "10024"]
mod auth;
mod discord;

use anyhow::Ok;
use anyhow::Result;
use async_recursion::async_recursion;
use auth::validate;
use azalea::block::{BlockState, BlockTrait};
use azalea::core::game_type::GameMode;
use azalea::local_player::LocalGameMode;
use azalea::prelude::*;
use azalea::protocol::packets::game::ClientboundEntityEvent;
use azalea::protocol::packets::game::{
    ClientboundGamePacket, ServerboundSetCommandBlock, s_set_command_block::Mode,
};
use azalea::{BlockPos, Client};
use discord::webhook_send;
use parking_lot::Mutex;
use rand::Rng;
use rand::rng;
use regex::Regex;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let mut username = vec![];

    username.extend(('A'..='Z').collect::<Vec<char>>());
    username.extend(('a'..='z').collect::<Vec<char>>());
    username.extend(('0'..='9').collect::<Vec<char>>());

    let mut rng = fastrand::Rng::new();
    let mut random_str = String::new();

    for _ in 0..6 {
        let idx = rng.usize(0..username.len());
        random_str.push(username[idx]);
    }

    let server = std::env::var("SERVER").unwrap_or_else(|_| "kaboom.pw".to_string());

    let _state = State {
        core_generator: Arc::new(tokio::sync::Mutex::new(Core::default())),
        used_codes: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        auth_users: Arc::new(Mutex::new(Vec::new())),
        loops: Arc::new(Mutex::new(Vec::new())),
        loops_text: Arc::new(Mutex::new(Vec::new())),
    };

    let account = Account::offline(&random_str);

    ClientBuilder::new()
        .set_handler(handle)
        .start(account, server)
        .await;

    Ok(())
}

#[derive(Default, Clone, Component)]
pub struct State {
    pub core_generator: Arc<tokio::sync::Mutex<Core>>,
    pub used_codes: Arc<tokio::sync::Mutex<Vec<u32>>>,
    pub auth_users: Arc<Mutex<Vec<String>>>,
    pub loops: Arc<Mutex<Vec<Arc<AtomicI32>>>>,
    pub loops_text: Arc<Mutex<Vec<String>>>,
}

#[derive(Default, Clone)]
pub struct Core {
    pub core_coordinates: Vec<BlockPos>,
}

impl Core {
    pub async fn gen_core(&mut self, bot: &Client, state: &State) -> anyhow::Result<Vec<BlockPos>> {
        let bot_pos = bot.position().to_block_pos_ceil();
        let min_pos = BlockPos::new(bot_pos.x, 0, bot_pos.z);
        let max_pos = BlockPos::new(bot_pos.x + 5, 2, bot_pos.z + 2);

        self.core_coordinates = BlockPos::between_closed(min_pos, max_pos);
        let command = format!(
            "/fill {} {} {} {} {} {} command_block",
            bot_pos.x,
            0,
            bot_pos.z,
            bot_pos.x + 5,
            2,
            bot_pos.z + 2
        );

        execute(bot, &command, state).await?;
        Ok(self.core_coordinates.clone())
    }

    pub fn gen_core_sync(&mut self, bot: &Client) -> anyhow::Result<Vec<BlockPos>> {
        let bot_pos = bot.position().to_block_pos_ceil();
        let min_pos = BlockPos::new(bot_pos.x, 0, bot_pos.z);
        let max_pos = BlockPos::new(bot_pos.x + 5, 2, bot_pos.z + 2);

        self.core_coordinates = BlockPos::between_closed(min_pos, max_pos);
        let command = format!(
            "/fill {} {} {} {} {} {} command_block",
            bot_pos.x,
            0,
            bot_pos.z,
            bot_pos.x + 5,
            2,
            bot_pos.z + 2
        );
        bot.chat(&command);

        Ok(self.core_coordinates.clone())
    }
}

#[async_recursion]
async fn execute(bot: &Client, command: &String, state: &State) -> anyhow::Result<()> {
    let coords = {
        let guard = state.core_generator.lock().await;
        guard.core_coordinates.clone()
    };

    let mut core = state.core_generator.lock().await;

    let random_pos: Option<BlockPos> = if !coords.is_empty() {
        let mut rng = rng();
        let random_index = rng.random_range(0..coords.len());
        Some(coords[random_index])
    } else {
        println!("no coords, dumbass!");
        None
    };

    fn is_bot_near(bot: &Client, target: BlockPos, max_distance: f64) -> bool {
        let bot_pos = bot.position().to_block_pos_ceil();
        bot_pos.distance_to(target) <= max_distance
    }

    if let Some(pos) = random_pos {
        if !is_bot_near(bot, pos, 1000 as f64) {
            println!("could not exec bc not cblock");
            let _ = core.gen_core_sync(bot);
            return Ok(());
        }
    }

    if let Some(pos2) = random_pos {
        let block = bot
            .world()
            .read()
            .get_block_state(pos2)
            .unwrap_or(BlockState::default());

        let needs_regen = {
            let block_trait: Box<dyn BlockTrait> =
                Box::<dyn azalea::block::BlockTrait>::from(block);
            !block_trait.id().to_string().contains("command_block")
        };

        if needs_regen {
            let _ = core.gen_core_sync(bot);
            return Ok(());
        } else {
        }
        let p = ServerboundSetCommandBlock {
            pos: pos2,
            command: command.to_string(),
            mode: Mode::Auto,
            track_output: true,
            conditional: false,
            automatic: true,
        };
        bot.write_packet(p.clone());
        println!("{:?}", &p);
        Ok(())
    } else {
        println!("no coord!");
        Err(anyhow::anyhow!("No coords"))
    }
}

async fn tellraw(
    bot: &Client,
    message: &String,
    coords: &Vec<BlockPos>,
    core: Option<Core>,
    state: &State,
) -> anyhow::Result<()> {
    let tellraw = format!(
        r#"minecraft:tellraw @a [{{"text":"[","color":"{c1}","italic":false}},{{"text":"n","color":"{c2}","italic":false}},{{"text":"e","color":"{c3}","italic":false}},{{"text":"o","color":"{c4}","italic":false}},{{"text":"]","color":"{c5}","italic":false}},{{"text":" {msg}","color":"white"}}]"#,
        c1 = "#54daf4",
        c2 = "#5ed0e8",
        c3 = "#68c6dc",
        c4 = "#72bcd0",
        c5 = "#8aa8c4",
        msg = &message
    );

    execute(bot, &tellraw, state).await?;

    Ok(())
}

async fn login(
    uuid: String,
    code: String,
    codes: &mut Vec<u32>,
    _users: &mut Vec<String>,
    state: &State,
) -> bool {
    let mut codes = {
        let guard = state.used_codes.lock().await;
        guard.clone()
    };

    if validate(&format!("{}", code), &mut codes) {
        let _ = {
            let mut guard = state.auth_users.lock();
            guard.push(uuid)
        };
        add_to_used_codes(code.parse::<u32>().unwrap_or(0), state).await;
        true
    } else {
        false
    }
}

struct CbLoop {
    id_ref: Arc<AtomicI32>,
    command: String,
    delay: u64,
    string: String,
}

impl CbLoop {
    async fn start(&self, bot: &Client, coords: &Vec<BlockPos>, state: &State) -> Result<()> {
        loop {
            if self.id_ref.load(Ordering::Relaxed) < 0 {
                return Err(anyhow::anyhow!("must be positive"));
            }
            execute(bot, &self.command, state).await?;
            tokio::time::sleep(Duration::from_millis(self.delay)).await;
        }
    }
}

async fn new_loop(
    bot: Arc<Client>,
    coords: Arc<Vec<BlockPos>>,
    command: String,
    delay: u64,
    state: &State,
) -> (i32, Arc<AtomicI32>, String) {
    if id <= 0 {
        println!("dumbass id must be positive");
        return (-1, Arc::new(AtomicI32::new(-1)), "".to_string());
    }

    let highest_id = {
        let guard = state.loops.lock();
        guard.clone().len() + 1
    };

    let id_ref = Arc::new(AtomicI32::new(highest_id as i32));
    let text: String = format!(
        "ID: {}, Command: {}, Delay: {}",
        &highest_id, &command, &delay
    );

    let cb_loop = CbLoop {
        id_ref: id_ref.clone(),
        command: command,
        delay: delay,
        string: text.clone(),
    };

    let clone_state = state.clone();
    tokio::spawn(async move { cb_loop.start(&bot, &coords, &clone_state).await });
    (highest_id as i32, id_ref, text)
}

fn stop_loop(id_ref: Arc<AtomicI32>) {
    id_ref.store(-1, Ordering::Relaxed);
}

enum BotCommand {
    Help,
    Info,
    Core,
    Loop(String, String, String),
    Execute(String),
    Tellraw(String),
    Login(String),
    Kick(String),
    Disable(String),
    Loops,
    Light(String),
}

impl BotCommand {
    fn parse(parts: &[&str]) -> Self {
        let command = parts[0];
        let args = parts[1..].join(" ");

        match command {
            "help" => BotCommand::Help,
            "info" => BotCommand::Info,
            "core" => BotCommand::Core,
            "exe" => BotCommand::Execute(args),
            "loop" => BotCommand::Loop(
                parts[1].to_string(),
                parts.get(2).unwrap_or(&"0").to_string(),
                parts.get(3..).unwrap_or(&[]).join(" ").to_string(),
            ),
            "kick" => BotCommand::Kick(args),
            "tellraw" => BotCommand::Tellraw(args),
            "login" => BotCommand::Login(args),
            "loops" => BotCommand::Loops,
            "light" => BotCommand::Light(args),
            "disable" => BotCommand::Disable(args),
            _ => BotCommand::Help,
        }
    }

    fn requires_auth(&self) -> bool {
        match self {
            BotCommand::Core | BotCommand::Loop(_, _, _) => true,
            _ => false,
        }
    }

    async fn execute(
        &self,
        state: &State,
        bot: &Client,
        coords: &Vec<BlockPos>,
        sender_uid: String,
        codes: &mut Vec<u32>,
        authenticated: &mut Vec<String>,
        mut core: Core,
    ) -> anyhow::Result<()> {
        let mut loops = {
            let guard = state.loops.lock();
            guard.clone()
        };
        let mut loops_text = {
            let guard = state.loops_text.lock();
            guard.clone()
        };

        match &self {
            BotCommand::Core => {
                //core.gen_core(bot, state).await?;
            }

            BotCommand::Info => {
                tellraw(
                    bot,
                    &"https://discord.gg/9zSgbreY".to_string(),
                    coords,
                    None,
                    state,
                )
                .await;
            }

            BotCommand::Disable(user) => {
                tellraw(bot, &"Coming soon".to_string(), coords, None, state).await;
            }

            BotCommand::Light(user) => {
                let command = format!("effect give {} night_vision infinite 0 true", user);
                execute(bot, &command, state).await;
            }

            BotCommand::Loop(action, delay, command) => {
                if loops
                    .iter()
                    .any(|h| h.load(Ordering::Relaxed) == delay.parse::<i32>().unwrap())
                    && action != "stop"
                {
                    tellraw(
                        bot,
                        &"A loop with that id already exists!".to_string(),
                        coords,
                        Some(core.clone()),
                        state,
                    )
                    .await?;

                    return Ok(());
                }
                if action == "start" {
                    println!("I JUST TRIED TO START A LOOP!!!!!");

                    let (id_num, handle, text) = new_loop(
                        Arc::new(bot.clone()),
                        Arc::new(coords.clone()),
                        command.clone(),
                        delay.parse::<u64>().unwrap(),
                        state,
                    )
                    .await;

                    let _ = {
                        let mut loops = state.loops.lock();
                        let mut loop_text = state.loops_text.lock();
                        loops.push(handle.clone());
                        loop_text.push(text);
                    };
                    tellraw(
                        bot,
                        &format!("Started loop {}", id_num),
                        coords,
                        Some(core),
                        state,
                    )
                    .await?;
                } else if action == "stop" {
                    let target_id = delay.parse::<i32>().unwrap();

                    if let Some(pos) = loops
                        .iter()
                        .position(|h| h.load(Ordering::Relaxed) == target_id)
                    {
                        stop_loop(loops[pos].clone());
                        loops.remove(pos);
                        loops_text.remove(pos);

                        let _ = {
                            let mut guard = state.loops_text.lock();
                            let mut guard2 = state.loops.lock();
                            guard.remove(pos);

                            guard2.remove(pos);
                        };
                        tellraw(
                            bot,
                            &format!("Stopped loop {}", target_id),
                            coords,
                            Some(core.clone()),
                            state,
                        )
                        .await?;
                    }
                }
            }

            BotCommand::Execute(cmd) => {
                let coords_upd = {
                    let guard = state.core_generator.lock().await;
                    guard.core_coordinates.clone()
                };

                let core_upd = {
                    let guard = state.core_generator.lock().await;
                    guard.clone()
                };

                println!("{:?}", coords_upd);
                tellraw(
                    bot,
                    &format!("running {}", cmd),
                    &coords_upd,
                    Some(core_upd.clone()),
                    state,
                )
                .await?;
                execute(bot, &cmd, state).await?;
            }

            BotCommand::Loops => {
                for lp in loops_text.iter() {
                    println!("{}", lp);
                    tellraw(bot, &*lp, coords, Some(core.clone()), state).await?;
                }
            }

            BotCommand::Help => {
                tellraw(
                    bot,
                    &"Requires authentication:\nloop, core, disable\n".to_string(),
                    coords,
                    Some(core),
                    state,
                )
                .await?;

                tellraw(
                    bot,
                    &"Public:\nexe, help, info, light".to_string(),
                    coords,
                    None,
                    state,
                )
                .await?;
            }

            BotCommand::Kick(user) => {
                let repeated = "L".repeat(20000);
                let custom_name = format!(
                    "/give {} diamond_hoe[minecraft:custom_name='ඞ{}']",
                    user, repeated
                );
                execute(bot, &custom_name, state).await?;
            }

            BotCommand::Tellraw(msg) => {
                tellraw(bot, &msg, coords, Some(core), state).await?;
            }

            BotCommand::Login(code) => {
                if login(sender_uid, code.clone(), &mut vec![], authenticated, state).await {
                    tellraw(
                        bot,
                        &"Succesfully authenticated".to_string(),
                        coords,
                        Some(core),
                        state,
                    )
                    .await?;
                } else {
                    tellraw(
                        bot,
                        &"Couldn't authenticate".to_string(),
                        coords,
                        Some(core),
                        state,
                    )
                    .await?;
                }
            }
        }

        return Ok(());
    }
}

async fn add_to_used_codes(code: u32, state: &State) {
    let _ = {
        let mut guard = state.used_codes.lock().await;
        guard.push(code)
    };
}

async fn handle(bot: Client, event: Event, state: State) -> anyhow::Result<()> {
    let mut authenticated = {
        let guard = state.auth_users.lock();
        guard.clone()
    };

    let core = {
        let core_guard = state.core_generator.lock().await;
        core_guard.clone()
    };

    let mut codes = {
        let guard = state.used_codes.lock().await;
        guard.clone()
    };

    match event {
        Event::Chat(m) => {
            let message = m.content();

            let op_regex = Regex::new(r"Made \b\w+\b no longer a server operator").unwrap();

            if let Some(mat) = op_regex.find(&message) {
                bot.chat(format!("/op {}", bot.username()));
            }

            let game_mode = *bot.component::<LocalGameMode>();

            if game_mode.current != GameMode::Creative {
                bot.chat("/gmc")
            }

            if !message.contains("Command set: ") {
                webhook_send(format!("{}", m.message())).await;
            }

            if message.starts_with("n:") {
                let parts: Vec<&str> = message[2..].trim().split_whitespace().collect();
                let command = BotCommand::parse(&parts);
                let sender_uuid = m.sender_uuid().unwrap().to_string();
                let coords = {
                    let guard = state.core_generator.lock().await;
                    guard.core_coordinates.clone()
                };
                if command.requires_auth() && !authenticated.contains(&sender_uuid) {
                    tellraw(
                        &bot,
                        &"Woah, login first!".to_string(),
                        &coords,
                        Some(core.clone()),
                        &state,
                    )
                    .await?;
                    return Ok(());
                }

                command
                    .execute(
                        &state,
                        &bot,
                        &coords,
                        sender_uuid.clone(),
                        &mut codes,
                        &mut authenticated,
                        core.clone(),
                    )
                    .await?;
                println!("tried running smth");
                return Ok(());
            } else {
                return Ok(());
            }
        }
        Event::Spawn => {
            println!("Bot connected \n\n\n\n");
            let _ = {
                let mut guard = state.core_generator.lock().await;
                guard.gen_core_sync(&bot);
            };
            Ok(())
        }

        Event::RemovePlayer(p) => {
            let _ = {
                let mut authed = state.auth_users.lock();

                if authed.contains(&p.uuid.to_string()) {
                    authed.retain(|x| x != &p.uuid.to_string());
                };
            };

            return Ok(());
        }

        Event::Packet(packet) => match &*packet {
            ClientboundGamePacket::BlockUpdate(p) => {
                let coords = {
                    let guard = state.core_generator.lock().await;
                    guard.core_coordinates.clone()
                };

                let needs_regen = {
                    let block_trait = Box::<dyn azalea::block::BlockTrait>::from(p.block_state);
                    coords.contains(&p.pos) && !block_trait.id().ends_with("command_block")
                };

                if needs_regen {
                    safe_core_gen(&bot, &state).await;
                }

                Ok(())
            }

            ClientboundGamePacket::EntityEvent(ClientboundEntityEvent {
                entity_id,
                event_id,
            }) => {
                if *event_id == 24 as u8 {
                    bot.chat("/op @s[type=player]");
                }
                Ok(())
            }

            _ => Ok(()),
        },

        _ => Ok(()),
    }
}

async fn safe_core_gen(bot: &Client, state: &State) {
    let _ = {
        let mut guard = state.core_generator.lock().await;
        let _ = guard.gen_core(bot, state).await;
        ()
    };
}
