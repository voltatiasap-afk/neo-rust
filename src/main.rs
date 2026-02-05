#![recursion_limit = "10024"]

mod auth;
mod discord;

use anyhow::Ok;
use anyhow::Result;
use async_recursion::async_recursion;
use auth::validate;
use azalea::block::BlockState;
use azalea::core::game_type::GameMode;
use azalea::local_player::LocalGameMode;
use azalea::prelude::*;
use azalea::protocol::packets::game::ClientboundEntityEvent;
use azalea::protocol::packets::game::{
    ClientboundGamePacket, ServerboundSetCommandBlock, s_set_command_block::Mode,
};
use azalea::registry::builtin::BlockKind;
use azalea::{BlockPos, Client};
use discord::webhook_send;
use parking_lot::Mutex;
use rand::Rng;
use rand::rng;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

#[tokio::main]
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
            println!("regenerated because bot is far away from the core");
            let _ = {
                let mut core = state.core_generator.lock().await;
                core.gen_core_sync(bot)?;
            };
        }

        let block = bot
            .world()
            .read()
            .get_block_state(pos)
            .unwrap_or(BlockState::default());

        let block_kind: BlockKind = block.into();
        if block_kind != BlockKind::CommandBlock
            && block_kind != BlockKind::RepeatingCommandBlock
            && block_kind != BlockKind::ChainCommandBlock
        {
            let _ = {
                let mut core = state.core_generator.lock().await;
                core.gen_core_sync(bot)?;
            };
        }

        let p = ServerboundSetCommandBlock {
            pos: pos,
            command: command.to_string(),
            mode: Mode::Auto,
            track_output: true,
            conditional: false,
            automatic: true,
        };
        bot.write_packet(p.clone());
        return Ok(());
    }

    Ok(())
}

async fn tellraw(bot: &Client, message: &String, state: &State) -> anyhow::Result<()> {
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

async fn login(uuid: String, code: String, _users: &mut Vec<String>, state: &State) -> bool {
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
}

impl CbLoop {
    async fn start(&self, bot: &Client, state: &State) -> Result<()> {
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
    command: String,
    delay: u64,
    state: &State,
) -> (i32, Arc<AtomicI32>, String) {
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
    };

    let clone_state = state.clone();
    tokio::spawn(async move { cb_loop.start(&bot, &clone_state).await });
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
    List,
    Light(String),
    AdvancedTellraw(String),
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
            "list" => BotCommand::List,
            "kick" => BotCommand::Kick(args),
            "tellraw" => BotCommand::Tellraw(args),
            "login" => BotCommand::Login(args),
            "loops" => BotCommand::Loops,
            "light" => BotCommand::Light(args),
            "disable" => BotCommand::Disable(args),
            "tellraw+" => BotCommand::AdvancedTellraw(args),
            _ => BotCommand::Help,
        }
    }

    fn requires_auth(&self) -> bool {
        match self {
            BotCommand::Core | BotCommand::Loop(_, _, _) => true,
            _ => false,
        }
    }

    async fn execute(&self, state: &State, bot: &Client, sender_uid: String) -> anyhow::Result<()> {
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

            BotCommand::List => {
                advanced_tellraw(
                    bot,
                    state,
                    &"<gray>Players <gray>currently <gray>online:".to_string(),
                )
                .await?;
                execute(bot, &"sudo * c:*".to_string(), state).await?;
            }

            BotCommand::AdvancedTellraw(message) => {
                advanced_tellraw(bot, state, message).await?;
            }

            BotCommand::Info => {
                advanced_tellraw(
                    bot,
                    state,
                    &"<blue>Join <blue>the <blue>discord: [https://discord.gg/9zSgbreY]"
                        .to_string(),
                )
                .await?;
            }

            BotCommand::Disable(_user) => {
                tellraw(bot, &"Coming soon".to_string(), state).await?;
            }

            BotCommand::Light(user) => {
                let command = format!("effect give {} night_vision infinite 0 true", user);
                execute(bot, &command, state).await?;
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
                        state,
                    )
                    .await?;

                    return Ok(());
                }
                if action == "start" {
                    let (id_num, handle, text) = new_loop(
                        Arc::new(bot.clone()),
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
                    tellraw(bot, &format!("Started loop {}", id_num), state).await?;
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
                        tellraw(bot, &format!("Stopped loop {}", target_id), state).await?;
                    }
                }
            }

            BotCommand::Execute(cmd) => {
                tellraw(bot, &format!("running {}", cmd), state).await?;
                execute(bot, &cmd, state).await?;
            }

            BotCommand::Loops => {
                for lp in loops_text.iter() {
                    println!("{}", lp);
                    tellraw(bot, &*lp, state).await?;
                }
            }

            BotCommand::Help => {
                advanced_tellraw(
                    bot,
                    state,
                    &"<gray>Commands <gray>(<aqua>8<gray>) <gray>- <aqua>info, <aqua>help, <aqua>exe, <aqua>tellraw, <aqua>login, <aqua>loops, <yellow>core, <yellow>loop".to_string(),

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
                tellraw(bot, &msg, state).await?;
            }

            BotCommand::Login(code) => {
                if login(sender_uid, code.clone(), &mut vec![], state).await {
                    tellraw(bot, &"Succesfully authenticated".to_string(), state).await?;
                } else {
                    tellraw(bot, &"Couldn't authenticate".to_string(), state).await?;
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
    let authenticated = {
        let guard = state.auth_users.lock();
        guard.clone()
    };

    match event {
        Event::Chat(m) => {
            let message = m.content();

            if message.starts_with("n:") {
                let parts: Vec<&str> = message[2..].trim().split_whitespace().collect();
                let command = BotCommand::parse(&parts);
                let sender_uuid = m.sender_uuid().unwrap().to_string();
                if command.requires_auth() && !authenticated.contains(&sender_uuid) {
                    tellraw(&bot, &"Woah, login first!".to_string(), &state).await?;
                    return Ok(());
                }

                command.execute(&state, &bot, sender_uuid.clone()).await?;
            } else {
            }
            let game_mode = *bot.component::<LocalGameMode>();

            if game_mode.current != GameMode::Creative {
                bot.chat("/gmc")
            }

            if !message.contains("Command set: ") {
                webhook_send(format!("{}", m.message())).await;
            }

            Ok(())
        }
        Event::Spawn => {
            println!("Bot connected \n\n\n\n");
            let _ = {
                let mut guard = state.core_generator.lock().await;
                guard.gen_core_sync(&bot)?;
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
                let block_kind: BlockKind = p.block_state.into();
                let (coords, mut core) = {
                    let guard = state.core_generator.lock().await;
                    (guard.core_coordinates.clone(), guard.clone())
                };

                let needs_regen = {
                    coords.contains(&p.pos)
                        && block_kind != BlockKind::RepeatingCommandBlock
                        && block_kind != BlockKind::CommandBlock
                        && block_kind != BlockKind::ChainCommandBlock
                };

                if needs_regen {
                    core.gen_core(&bot, &state).await?;
                } else {
                }

                Ok(())
            }

            ClientboundGamePacket::EntityEvent(ClientboundEntityEvent {
                entity_id: _,
                event_id,
            }) => {
                if event_id.clone() == 24 as u8 {
                    bot.chat("/op @s[type=player]");
                }
                Ok(())
            }

            _ => Ok(()),
        },

        _ => Ok(()),
    }
}

async fn advanced_tellraw(bot: &Client, state: &State, message: &String) -> Result<()> {
    let mut output: Vec<String> = vec![];

    let mut colors: HashMap<&str, &str> = HashMap::new();

    colors.insert("<r>", "red");
    colors.insert("<b>", "blue");
    colors.insert("<c>", "aqua");
    colors.insert("<y>", "yellow");
    colors.insert("<g>", "green");
    colors.insert("<j>", "gray");
    colors.insert("<w>", "white");

    //kill me please
    let regex = regex::Regex::new(r#"(?:\[([^\]]*)\])?(?:<([^>]*)*>)?([^<\[]*)"#)?;
    for cap in regex.captures_iter(message) {
        //TODO add logic here
        let links = cap.get(1).map_or("", |m| m.as_str());
        let color = cap.get(2).map_or("", |m| m.as_str());
        let text = cap.get(3).map_or("", |m| m.as_str());

        if !links.is_empty() {
            output.push(format!(
            r#"{{"text":"{}","color":"aqua","click_event":{{"action":"open_url","url":"{}"}}}}"#,
            links, links.trim()
        ));
        }
        if color.is_empty() {
            output.push(format!(r#"{{"text":"{}","color":"white"}}"#, text));
        }
        if !color.is_empty() {
            output.push(format!(r#"{{"text":"{}","color":"{}"}}"#, text, color));
        }
    }
    // for word in message.split(&[' ', '*'][..]).filter(|w| !w.is_empty()) {
    //     if word.starts_with("<") && word.len() > 3 {
    //         let text = &word[3..];
    //         let tag = &word[..3];
    //         if let Some(color) = colors.get(tag) {
    //             output.push(
    //                 format!(r#"{{"text":"{} ", "color":"{}"}}"#, text.to_string(), color)
    //                     .to_string(),
    //             );
    //         }
    //     } else {
    //         output.push(
    //             format!(r#"{{"text":"{} ", "color":"white"}}"#, word.to_string()).to_string(),
    //         );
    //     }
    // }

    execute(bot, &format!("/tellraw @a [{}]", output.join(",")), state).await?;

    Ok(())
}
