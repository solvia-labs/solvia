//! named accounts for synthesized data accounts for bank state, etc.
//!
//! this account carries history about stake activations and de-activations
//!
pub use crate::fnode_data::FNodeData;

pub use crate::{
    account_info::AccountInfo, program_error::ProgramError, slot_history::SlotHistory,
};

use crate::sysvar::Sysvar;

crate::declare_sysvar_id!("SysvarFNodeData1111111111111111111111111111", FNodeData);

impl Sysvar for FNodeData {
    // override
//    fn size_of() -> usize {
//        // hard-coded so that we don't have to construct an empty
//        16392 // golden, update if MAX_ENTRIES changes
//    }
    fn from_account_info(_account_info: &AccountInfo) -> Result<Self, ProgramError> {
        // This sysvar is too large to bincode::deserialize in-program
        Err(ProgramError::UnsupportedSysvar)
    }


}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fnode_data::*;

    #[test]
    fn test_size_of() {
//        let mut stake_history = StakeHistory::default();
//        for i in 0..MAX_ENTRIES as u64 {
//            stake_history.add(
//                i,
//                StakeHistoryEntry {
//                    activating: i,
//                    ..StakeHistoryEntry::default()
//                },
//            );
//        }
//
//        assert_eq!(
//            bincode::serialized_size(&stake_history).unwrap() as usize,
//            StakeHistory::size_of()
//        );
    }
//
//    #[test]
//    fn test_create_account() {
//        let mut stake_history = StakeHistory::default();
//        for i in 0..MAX_ENTRIES as u64 + 1 {
//            stake_history.add(
//                i,
//                StakeHistoryEntry {
//                    activating: i,
//                    ..StakeHistoryEntry::default()
//                },
//            );
//        }
//        assert_eq!(stake_history.len(), MAX_ENTRIES);
//        assert_eq!(stake_history.iter().map(|entry| entry.0).min().unwrap(), 1);
//        assert_eq!(stake_history.get(&0), None);
//        assert_eq!(
//            stake_history.get(&1),
//            Some(&StakeHistoryEntry {
//                activating: 1,
//                ..StakeHistoryEntry::default()
//            })
//        );
//    }
}
