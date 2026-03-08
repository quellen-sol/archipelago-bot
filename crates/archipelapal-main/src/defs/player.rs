use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{
    items::{Effect, Item},
    lib::{ItemID, RegionID},
};

/// Speed boost modifier percentage (1%).
pub const SPEED_BOOST_MODIFIER_PCT: f32 = 0.01;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerState {
    /// K = ItemID, V = qty
    pub inventory: HashMap<ItemID, u16>,
    // pub checked_locations: HashSet<LocationID>,
    pub currently_exploring_region: RegionID,
    pub speed_modifier: f32,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            inventory: HashMap::new(),
            currently_exploring_region: 0,
            speed_modifier: 1.0,
        }
    }
}

impl PlayerState {
    pub fn get_accessible_regions(&self) -> Vec<RegionID> {
        self.inventory
            .iter()
            .filter_map(|(id, _)| {
                let item = Item::from_id(*id)?;
                match item {
                    Item::Key(region) => Some(region),
                    _ => None,
                }
            })
            .collect()
    }

    pub fn get_num_goal_items(&self) -> u16 {
        self.inventory
            .iter()
            .filter_map(|(id, amt)| {
                let item = Item::try_from_le_bytes(&id.to_le_bytes())?;
                match item {
                    Item::Goal => Some(*amt),
                    _ => None,
                }
            })
            .sum::<u16>()
    }

    pub fn get_num_boosts(&self) -> u16 {
        self.inventory
            .iter()
            .filter_map(|(id, amt)| {
                let item = Item::from_id(*id)?;
                match item {
                    Item::GameAffector(Effect::SpeedBoost) => Some(*amt),
                    _ => None,
                }
            })
            .sum::<u16>()
    }

    pub fn get_total_speed_modifier(&self) -> f32 {
        // Testing multiple types of modifiers
        let speed_boosts = self.get_num_boosts();
        // Simple stacking 1%

        // Exponential 1%
        // let modifier = (1.0 + SPEED_BOOST_MODIFIER_PCT).powf(speed_boosts as f32);

        speed_boosts as f32 * SPEED_BOOST_MODIFIER_PCT + 1.0
    }

    pub fn set_speed_modifier(&mut self) {
        let modifier = self.get_total_speed_modifier();
        log::info!("Speed modifier set to: {}", modifier);
        self.speed_modifier = modifier;
    }
}
