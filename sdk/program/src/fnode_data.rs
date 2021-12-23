//! Provides information about the Fundamental Nodes which consists of Collateral, ROIs, TotalPaid, etc

use crate::pubkey::Pubkey;
use std::{iter::FromIterator};
use std::ops::Deref;

/// FNodeData represents FundamentalNode data.  Members of Clock start from 0 upon
///  network boot.  The best way to map Clock to wallclock time is to use
///  current Slot, as Epochs vary in duration (they start short and grow
///  as the network progresses).
///
pub type RewardAddress = Pubkey;
pub type NodeType = i8;
pub type TotalPaid = u64;
pub type State = bool;

// Constants
// Collaterals
pub const COLLATERAL_PHOENIX: f64 = 20000.0;
pub const COLLATERAL_NOUA: f64 = 5000.0;
pub const COLLATERAL_FULGUR: f64 = 2000.0;
// Max roi to give(unit - tokens)
pub const MAX_ROI_PHOENIX: f64 = 30000.0;
pub const MAX_ROI_NOUA: f64 = 7000.0;
pub const MAX_ROI_FULGUR: f64 = 2600.0;
//// Rewards for each category per epoch(432k blocks) (unit - tokens)
pub const PER_EPOCH_REWARD_PHOENIX: f64 = 14400.0;
pub const PER_EPOCH_REWARD_NOUA: f64 = 8640.0;
pub const PER_EPOCH_REWARD_FULGUR: f64 = 5760.0;


pub type NodeData = (RewardAddress, NodeType, TotalPaid, State);
#[repr(C)]
#[derive(Serialize, Deserialize, PartialEq, Debug, Default)]
pub struct FNodeData(Vec<NodeData>);
// todo : add add() to add fields..... imp--remove from_account_info bincode cant deser large size
// todo : make vector or array of FNodeData

impl FNodeData {
    pub fn replace_with(&mut self, new_node_vec: Vec<(RewardAddress, NodeType, TotalPaid, State)>) {
        (self.0).clear();
        let mut index =0;
        while index < new_node_vec.len(){
            (self.0).push(new_node_vec[index]);
            index+=1;
        }
    }
    pub fn add(&mut self, new_fnode : NodeData) {
        //let index = (self.0).len();
        (self.0).push(new_fnode);
    }

    pub fn new_frji(&mut self) {
        let new_fnode : NodeData = (Pubkey::default(), 0, 0 as u64, false);
        self.add(new_fnode);
    }
}

impl FromIterator<(RewardAddress, NodeType, TotalPaid, State)> for FNodeData {
    fn from_iter<I: IntoIterator<Item = (RewardAddress, NodeType, TotalPaid, State)>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl Deref for FNodeData {
    type Target = Vec<NodeData>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}