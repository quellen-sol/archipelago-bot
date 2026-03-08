use std::{sync::Arc, thread, time::Duration};

use anyhow::Context;
use ap_rs::{Connection, Permission};
use rand::{thread_rng, Rng};
use tokio::{
    sync::oneshot::{self, error::TryRecvError},
    task::JoinHandle,
};

use crate::defs::{game_state::FullGameState, lib::ArchipelaPalSlotData};

pub fn spawn_game_playing_task(
    game_state: Arc<FullGameState>,
    connection: Arc<Connection<ArchipelaPalSlotData>>,
    config: ArchipelaPalSlotData,
    mut goal_rx: oneshot::Receiver<()>,
    release_perms: Permission,
) {
    println!("Searching for items...");
    let max_wait_time = config.max_wait_time;
    let min_wait_time = config.min_wait_time;
    let client = connection.client_mut().unwrap();
    loop {
        let wait_time = {
            // `rng` must drop out of scope before entering back into async land
            let mut rng = thread_rng();
            rng.gen_range(min_wait_time..=max_wait_time)
        };

        let duration = {
            // Grab a read lock here, and release after finishing
            let player = game_state.player.read().unwrap();
            let speed_modifier = &player.speed_modifier;
            let wait_time = ((wait_time as f32 / speed_modifier) * 1000.0) as u64;
            let wait_time = wait_time.max(min_wait_time as u64 * 1000);
            log::info!("waiting for {wait_time} ms");
            Duration::from_millis(wait_time)
        };
        thread::sleep(duration);
        match goal_rx.try_recv() {
            Ok(_) => {
                // We goaled!! Send packet to server
                client.set_status(ap_rs::ClientStatus::Goal).unwrap();
                client.say("gg <3".into()).ok();

                // Check if we need to manually release
                if !matches!(release_perms, Permission::Auto | Permission::AutoEnabled) {
                    log::info!("Releasing items...");
                    println!("Releasing items...");
                    client
                        .say("!release".into())
                        .context("Could not send release items message")
                        .unwrap();
                } else {
                    log::info!("I do not have to manually release!");
                }

                game_state
                    .write_save_file()
                    .inspect_err(|e| log::error!("Error writing save file on goal: {e}"))
                    .ok();

                // End the thread :)
                log::info!("Shutting down gameplay thread");
                return;
            }
            Err(e) => match e {
                TryRecvError::Empty => {
                    // All good, we just haven't goaled yet.
                }
                TryRecvError::Closed => {
                    panic!("GOAL oneshot is poisoned!");
                }
            },
        };

        let location_checked = game_state.tick_game_state();

        match location_checked {
            None => {
                // BK'd!
                log::warn!("I'm BK'd!!!");
                println!("Currently in BK mode!");
            }
            Some(loc_id) => {
                // Found an item!
                println!("Checked location ID: {loc_id} (Hex: {loc_id:x})");
                let loc_id = loc_id as i64;
                match client.mark_checked([loc_id]) {
                    Ok(_) => {
                        // Remove from hint queue
                        // let mut source_hint_queue = game_state.source_hint_queue.write().await;
                        // source_hint_queue.retain(|hint| hint.item.location != loc_id);
                    }
                    Err(e) => {
                        log::error!("{e:?}");
                    }
                };
            }
        }

        let hint_get_key = game_state.make_hints_get_key(game_state.slot_id);
        client.get([hint_get_key]);
    }
}
