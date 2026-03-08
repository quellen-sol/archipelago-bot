use serde::{Deserialize, Serialize};

pub const SAVE_FILE_DIRECTORY: &str = "Saves";

pub type RegionID = u8;
pub type LocationID = u32;
pub type ItemID = u32;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ArchipelaPalSlotData {
    pub min_wait_time: u16,
    pub max_wait_time: u16,
    pub num_goal: u16,
    pub slot_name: String,
    pub num_regions: u8,
    pub chests_per_region_list: Vec<u8>,
    pub game_theme: u8,
}
