use std::{
    fs,
    io::{stdin, stdout, Write},
    path::PathBuf,
    sync::Arc,
    vec,
};

use anyhow::{anyhow, Context, Result};
use ap_rs::{client::ArchipelagoClient, protocol::Get};
use clap::Parser;
use defs::{Config, FullGameState, GameMap, GoalOneShotData};
use processes::{
    game_playing_thread::spawn_game_playing_task, message_handler::spawn_ap_server_task,
};
use tokio::sync::oneshot;

mod defs;
mod processes;

#[derive(Parser)]
struct Args {
    #[clap()]
    config_file: PathBuf,

    #[clap(long, short, env)]
    slot_name: Option<String>,

    #[clap(long, short = 'a', env)]
    server_addr: Option<String>,

    #[clap(long, short, env)]
    password: Option<String>,
}

pub const GAME_NAME: &str = "APBot";
pub const ITEM_HANDLING: i32 = 0b111;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    env_logger::init();

    let args = Args::parse();

    let addr = args
        .server_addr
        .unwrap_or_else(|| get_user_input("Enter server ip and port:").unwrap());
    let mut client =
        ArchipelagoClient::with_data_package(&addr, Some(vec![GAME_NAME.into()])).await?;

    // Load config file
    let config = fs::read_to_string(&args.config_file)
        .and_then(|file_s| serde_json::from_str::<Config>(&file_s).map_err(Into::into))?;

    log::info!("Loaded config {}", args.config_file.display());

    let password = args
        .password
        .unwrap_or_else(|| get_user_input("Enter server password (Press Enter if none):").unwrap());

    let slot_name = args
        .slot_name
        .unwrap_or_else(|| get_user_input("Enter slot name:").unwrap());

    let connected_packet = client
        .connect(
            GAME_NAME,
            &slot_name,
            Some(&password),
            Some(ITEM_HANDLING), // ?
            vec!["AP".into(), "Bot".into()],
        )
        .await?;

    log::info!("Connected");

    let team = connected_packet
        .players
        .iter()
        .find_map(|p| {
            if p.name == config.slot_name {
                Some(p.team)
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow!("No player in server list with name {}", config.slot_name))?;

    let this_game_data = client
        .data_package()
        .and_then(|dp| dp.games.get(GAME_NAME))
        .ok_or_else(|| anyhow!("Data package not preset for this game and slot???"))?;

    let info = client.room_info();
    log::info!("Seed: {}", info.seed_name);

    let mut game_state = FullGameState::from_file_or_default(&info.seed_name);

    // Correct the game state if it ended up being a default
    if game_state.seed_name.is_empty() {
        let loc_to_id = &this_game_data.location_name_to_id;
        let game_map = GameMap::new_from_data_package(loc_to_id);

        let mut map_lock = game_state.map.write().await;
        *map_lock = game_map;
        drop(map_lock);

        game_state.seed_name = info.seed_name.clone();
        game_state.team = team;
    }

    let game_state = Arc::new(game_state);

    let (mut client_sender, client_receiver) = client.split();

    let (goal_tx, goal_rx) = oneshot::channel::<GoalOneShotData>();

    // Spawn server listen thread
    let server_handle =
        spawn_ap_server_task(game_state.clone(), client_receiver, config.clone(), goal_tx);

    // Task started, slight delay, then send syncing packets
    client_sender
        .send(ap_rs::protocol::ClientMessage::Get(Get {
            keys: vec![format!("client_status_{team}_{}", config.slot_name)],
        }))
        .await
        .context("Failed to get my status!")?;

    client_sender
        .send(ap_rs::protocol::ClientMessage::Sync)
        .await
        .expect("Could not send sync packet!");

    let game_handle =
        spawn_game_playing_task(game_state.clone(), client_sender, config.clone(), goal_rx);

    server_handle.await.unwrap();
    game_handle.await.unwrap();

    Ok(())
}

fn get_user_input(prompt: &str) -> Result<String> {
    let mut buf = String::new();
    let sin = stdin();
    print!("{prompt}");
    stdout().flush()?;
    sin.read_line(&mut buf)?;

    Ok(buf.trim().to_string())
}
