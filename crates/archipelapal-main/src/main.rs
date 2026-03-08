use std::{
    fs,
    io::{stdin, stdout, Write},
    sync::Arc,
    thread,
    time::Duration,
    vec,
};

use anyhow::{anyhow, bail, Context, Result};
use ap_rs::{Client, ConnectionOptions, ItemHandling};
use clap::Parser;
use defs::{
    game_state::{FullGameState, GameMap},
    lib::{ArchipelaPalSlotData, SAVE_FILE_DIRECTORY},
    user_settings::UserSettings,
};
use processes::{
    game_playing_thread::spawn_game_playing_task, message_handler::spawn_ap_server_task,
};
use tokio::sync::oneshot;

mod defs;
mod processes;
mod utils;

#[derive(Parser)]
struct Args {
    #[clap(long, short, env)]
    slot_name: Option<String>,

    #[clap(long, short = 'a', env)]
    server_addr: Option<String>,

    #[clap(long, short, env)]
    password: Option<String>,

    #[clap(long)]
    skip_start_confirmation: bool,
}

pub const GAME_NAME: &str = "ArchipelaPal";
pub const ITEM_HANDLING: i32 = 0b111;

#[tokio::main]
async fn main() -> Result<()> {
    let main_result = inner_main().await;

    match main_result {
        Ok(_) => Ok(()),
        Err(e) => {
            log::error!("{e}");
            println!("{e}");
            get_user_input("Press Enter to exit...")?;
            bail!(e)
        }
    }
}

// This is our primary `main` function, but to allow for easy error handling and logging, we have
// the actual main function call this one.
async fn inner_main() -> Result<()> {
    dotenvy::dotenv().ok();
    env_logger::init();

    let args = Args::parse();
    let mut user_settings = UserSettings::load_or_default();

    let slot_name = args
        .slot_name
        .or_else(|| {
            let last_slot_name = &user_settings.last_used_slot;
            let prompt = match last_slot_name {
                Some(name) => format!("Enter slot name (Press Enter for last used: \"{name}\"):"),
                None => "Enter slot name:".to_string(),
            };
            let user_input = get_user_input(&prompt).unwrap();
            match (user_input, last_slot_name) {
                (input, _) if input.is_empty() => last_slot_name.clone(),
                (input, _) => Some(input),
            }
        })
        .ok_or_else(|| anyhow!("Slot name cannot be empty!"))?;

    user_settings.last_used_slot = Some(slot_name.clone());

    let addr = args
        .server_addr
        .or_else(|| {
            let last_server_addr = &user_settings.last_used_address;
            let prompt = match last_server_addr {
                Some(addr) => {
                    format!("Enter server address (Press Enter for last used: \"{addr}\"):")
                }
                None => "Enter server address:".to_string(),
            };
            let user_input = get_user_input(&prompt).unwrap();
            match (user_input, last_server_addr) {
                (input, _) if input.is_empty() => last_server_addr.clone(),
                (input, _) => Some(input),
            }
        })
        .ok_or_else(|| anyhow!("Server address cannot be empty!"))?;

    user_settings.last_used_address = Some(addr.clone());

    let password = args
        .password
        .unwrap_or_else(|| get_user_input("Enter server password (Press Enter if none):").unwrap());

    user_settings
        .save()
        .context("Could not save user settings")?;

    let mut conn = ap_rs::Connection::<ArchipelaPalSlotData>::new(
        &addr,
        &slot_name,
        Some(GAME_NAME),
        ConnectionOptions::new()
            .password(password)
            .receive_items(ItemHandling::OtherWorlds {
                own_world: true,
                starting_inventory: true,
            }),
    );

    while !conn.is_connected() {
        thread::sleep(Duration::from_secs(1));
        log::info!("Waiting for connection to AP server...");
    }
    let (config, seed_name, team, slot_id, release_perms) = {
        let client_new = conn.client_mut().context("no Client?")?;
        let release_perms = client_new.release_permission();
        let config = client_new.slot_data().clone();
        let seed_name = client_new.seed_name().to_string();
        let player = client_new.this_player();
        let team = player.team();
        let slot_id = player.slot();

        client_new
            .get([format!("_read_client_status_{team}_{slot_id}")])
            .await;
        client_new.sync()?;

        (config, seed_name, team, slot_id, release_perms)
    };

    let conn_arc = Arc::new(conn);

    log::debug!("Config: {config:?}");

    log::info!("Connected");

    log::info!("Seed: {}", seed_name);

    // Make 'Saves' directory if it doesn't exist
    fs::create_dir_all(SAVE_FILE_DIRECTORY).context("Could not create 'Saves' directory")?;

    let mut game_state = FullGameState::from_file_or_default(&seed_name);

    // Correct the game state if it ended up being a default
    if game_state.seed_name.is_empty() {
        let game_map = GameMap::new_from_config(&config);

        let mut map_lock = game_state.map.write().unwrap();
        *map_lock = game_map;
        drop(map_lock);

        // GAME STATE FIRST TIME CREATION
        game_state.seed_name = seed_name.to_string();
        game_state.team = team;
        game_state.slot_id = slot_id;
    }

    let game_state = Arc::new(game_state);

    let (goal_tx, goal_rx) = oneshot::channel::<()>();

    // Spawn server listen thread
    let server_handle = spawn_ap_server_task(
        game_state.clone(),
        conn_arc.clone(),
        config.clone(),
        goal_tx,
    );

    if !args.skip_start_confirmation {
        // Prompt user to start game "press enter to start"
        let start_prompt = format!("Press Enter to start {GAME_NAME} for slot {slot_name}...");
        get_user_input(&start_prompt)?;
    }

    let game_handle = spawn_game_playing_task(
        game_state.clone(),
        conn_arc,
        config.clone(),
        goal_rx,
        release_perms,
    );

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
