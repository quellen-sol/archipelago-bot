use anyhow::Result;
use ap_rs::{Client, Connection, Event};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    thread,
    time::Duration,
};
use tokio::{sync::oneshot, task::JoinHandle};

use crate::defs::{game_state::FullGameState, lib::ArchipelaPalSlotData};

pub fn spawn_ap_server_task(
    game_state: Arc<FullGameState>,
    connection: Arc<Connection<ArchipelaPalSlotData>>,
    config: ArchipelaPalSlotData,
    goal_tx: oneshot::Sender<()>,
) -> JoinHandle<()> {
    println!("Now listening for AP server messages");
    tokio::spawn(async move {
        let client = connection.client().unwrap();
        loop {
            tokio::time::sleep(Duration::from_millis(10)).await;
            for msg in connection.update() {
                match msg {
                    Event::ReceivedItems(index) => {
                        let items = client.received_items();
                        let mut player = game_state.player.write().unwrap();
                        let last_idx = game_state.last_checked_idx.read().unwrap();
                        if index == 0 {
                            // What we receive is the ENTIRE inventory when idx == 0
                            // Set the player's state and return
                            let new_player_inventory =
                                items.into_iter().fold(HashMap::new(), |mut acc, curr| {
                                    let id = curr.item().id();
                                    if id < 0 {
                                        return acc;
                                    }
                                    let amt = acc.entry(id as u32).or_insert(0);
                                    *amt += 1;

                                    acc
                                });

                            player.inventory = new_player_inventory;
                        } else if index > *last_idx {
                            for item in items.iter() {
                                let id = item.item().id();

                                if id < 0 {
                                    // Special AP item. don't use
                                    continue;
                                }

                                // Append to inventory for now...
                                let entry = player.inventory.entry(id as u32).or_insert(0);
                                *entry += 1;
                            }

                            // Drop read lock to get a write
                            drop(last_idx);

                            let mut last_idx_write = game_state.last_checked_idx.write().unwrap();
                            *last_idx_write = index;
                        }
                        player.set_speed_modifier();

                        let player = player.downgrade();
                        // Quick goal check
                        let player_goaled = player.get_num_goal_items() >= config.num_goal;

                        if player_goaled {
                            log::info!("GOOOOAAALLLLL");
                            goal_tx.send(()).unwrap();

                            // Need to more gracefully shutdown
                            log::info!("Server listening thread shutting down");
                            return;
                        }

                        game_state
                            .write_save_file()
                            .inspect_err(|e| log::error!("Unable to write save file: {e}"))
                            .ok();
                    }
                    // No retrieved variant??
                    // Event::Retrieved(retrieved) => {
                    //     for (key, val) in retrieved.keys.iter() {
                    //         if key.starts_with("_read_client_status") {
                    //             if !key.ends_with(&game_state.slot_id.to_string()) {
                    //                 continue;
                    //             }

                    //             if val.is_null() {
                    //                 continue;
                    //             }

                    //             let status: Option<ClientStatus> = val
                    //                 .as_number()
                    //                 .and_then(|n| n.as_u64())
                    //                 .map(|n64| (n64 as u16).into());

                    //             if let Some(ClientStatus::ClientGoal) = status {
                    //                 goal_tx.send(()).unwrap();

                    //                 // Need to more gracefully shutdown
                    //                 log::info!("Server listening thread shutting down");
                    //                 return;
                    //             }
                    //         } else if key.starts_with("_read_hints_") {
                    //             if val.is_null() {
                    //                 continue;
                    //             }

                    //             let Some(hints) = val.as_array() else {
                    //                 log::error!("Hints not an array?");
                    //                 continue;
                    //             };

                    //             let hints_parsed = hints
                    //                 .iter()
                    //                 .filter_map(|v| {
                    //                     let parsed: Result<HintData> =
                    //                         serde_json::from_value::<Hint>(v.clone())
                    //                             .map_err(Into::into)
                    //                             .map(|v| v.into());
                    //                     let Ok(hint_data) = parsed else {
                    //                         log::error!("Failed to parse hint: {v}");
                    //                         return None;
                    //                     };

                    //                     // We only care about hints from ourselves, for now
                    //                     if hint_data.item.player != game_state.slot_id
                    //                         || hint_data.found
                    //                         || !hint_data.is_important
                    //                     {
                    //                         return None;
                    //                     }

                    //                     Some(hint_data)
                    //                 })
                    //                 .collect::<HashSet<HintData>>();

                    //             let mut source_hint_queue = game_state.source_hint_queue.write().await;
                    //             *source_hint_queue = hints_parsed;
                    //         }
                    //     }
                    // }
                    // Event::Print(print_json) => {
                    //     if print_json.data().found.is_none() {
                    //         // Not a hint
                    //         continue;
                    //     }

                    //     let hint: HintData = print_json.into();

                    //     if hint.item.player == game_state.slot_id
                    //         && !hint.found
                    //         && hint.is_important
                    //     {
                    //         // This hint is an item that comes from us
                    //         let mut source_hint_queue = game_state.source_hint_queue.write().await;
                    //         source_hint_queue.insert(hint);
                    //     }
                    // }
                    _ => {
                        // Supporting other packet types as needed
                        continue;
                    }
                }
            }
        }
    })
}
