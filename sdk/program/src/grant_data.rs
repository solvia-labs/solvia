//! Provides information about the Fundamental Nodes which consists of Collateral, ROIs, TotalPaid, etc

use crate::pubkey::Pubkey;
use crate::hash::{Hash};
use std::ops::Deref;

/// FNodeData represents FundamentalNode data.  Members of Clock start from 0 upon
///  network boot.  The best way to map Clock to wallclock time is to use
///  current Slot, as Epochs vary in duration (they start short and grow
///  as the network progresses).

pub type GrantHash = Hash;
pub type ID = i16;
pub type ReceivingAddress = Pubkey;
pub type Amount = u64;
pub type VoteWeight = i32;
pub type FirstEpoch = u64;
pub type Votes = Vec<Pubkey>;

// Constants
// Vote Weight Node Category
pub const VOTES_PHOENIX : i32 = 13;
pub const VOTES_NOUA: i32 = 3;
pub const VOTES_FULGUR: i32 = 1;
pub const MAX_AMOUNT_PER_GRANT: f64 = 20000.0;
pub const MAX_GRANT_PER_MONTH: f64 =100000.0;

pub const GRANT_AUTH_PUBKEY: &str  = "8WFJt4rKLTnUhUZHFWjviUo6kPQyQLh9hyRZW5C53x5n";

pub type GrantData = (GrantHash, ID, ReceivingAddress, Amount, VoteWeight, FirstEpoch, Votes);
#[repr(C)]
#[derive(Serialize, Deserialize, PartialEq, Debug, Default)]
pub struct VecGrantData(Vec<GrantData>);
// todo : add add() to add fields..... imp--remove from_account_info bincode cant deser large size
// todo : make vector or array of FNodeData

impl VecGrantData {
    pub fn replace_with(&mut self, new_grant_data_vec: Vec<(GrantHash, ID, ReceivingAddress, Amount, VoteWeight, FirstEpoch, Votes)>) {
        (self.0).clear();
        let mut index =0;
        while index < new_grant_data_vec.len(){
            (self.0).push(new_grant_data_vec[index].clone());
            index+=1;
        }
    }
    pub fn add(&mut self, new_grant : GrantData) {
        //let index = (self.0).len();
        (self.0).push(new_grant);
    }

    pub fn new_frji(&mut self) {
        let vec_votes = vec![Pubkey::default()];
        let new_grant : GrantData = (Hash::new(&[0 as u8; 32]), 0, Pubkey::default(), 0 as u64, 0, 0 as u64, vec_votes.clone());
        if (self.0).len()!=0{
        (self.0).clear();}
        self.add(new_grant);
    }
}

//impl FromIterator<(RewardAddress, NodeType, TotalPaid, State)> for FNodeData {
//    fn from_iter<I: IntoIterator<Item = (RewardAddress, NodeType, TotalPaid, State)>>(iter: I) -> Self {
//        Self(iter.into_iter().collect())
//    }
//}

impl Deref for VecGrantData {
    type Target = Vec<GrantData>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}