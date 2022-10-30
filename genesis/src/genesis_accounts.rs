use crate::{
    stakes::{create_and_add_stakes, StakerInfo},
    unlocks::UnlockInfo,
};
use solana_sdk::{genesis_config::GenesisConfig, native_token::LAMPORTS_PER_SOL};

// 9 month schedule is 100% after 9 months
const UNLOCKS_ALL_AT_9_MONTHS: UnlockInfo = UnlockInfo {
    cliff_fraction: 1.0,
    cliff_years: 0.75,
    unlocks: 0,
    unlock_years: 0.0,
    custodian: "6VzcvNJGoCFoR2v8j5ynDSs5o3fWiBzvbeNEMyQmqHyr",
};

// 9 month schedule is 50% after 9 months, then monthly for 2 years
//const UNLOCKS_HALF_AT_9_MONTHS: UnlockInfo = UnlockInfo {
//    cliff_fraction: 0.5,
//    cliff_years: 0.75,
//    unlocks: 24,
//    unlock_years: 2.0,
//    custodian: "6VzcvNJGoCFoR2v8j5ynDSs5o3fWiBzvbeNEMyQmqHyr",
//};

// no lockups
const UNLOCKS_ALL_DAY_ZERO: UnlockInfo = UnlockInfo {
    cliff_fraction: 1.0,
    cliff_years: 0.0,
    unlocks: 0,
    unlock_years: 0.0,
    custodian: "6VzcvNJGoCFoR2v8j5ynDSs5o3fWiBzvbeNEMyQmqHyr",
};

//pub const CREATOR_STAKER_INFOS: &[StakerInfo] = &[
//    StakerInfo {
//        name: "impossible pizza",
//        staker: "uE3TVEffRp69mrgknYr71M18GDqL7GxCNGYYRjb3oUt",
//        lamports: 5_000_000 * LAMPORTS_PER_SOL,
//        withdrawer: Some("59SLqk4ete5QttM1WmjfMA7uNJnJVFLQqXJSy9rvuj7c"),
//    },
//];

pub const SERVICE_STAKER_INFOS: &[StakerInfo] = &[
    StakerInfo {
        name: "wretched texture",
        staker: "ALv8xLUVJL14Gzf7uAiRM52kHd6KwjTkofMnGk95CmeY",
        lamports: 145402737282702,
        withdrawer: Some("BpsVM1y7GFT6C6FiTzrPbxDZ3hK5ktZn7rwcrLigduKb"),
    },
];

pub const FOUNDATION_STAKER_INFOS: &[StakerInfo] = &[
//    StakerInfo {
//        name: "lyrical supermarket",
//        staker: "77GPhHvrUaTRuWPcXnnjCuaGLBm81XjpMZTjk5dLias",
//        lamports: 2_900_000 * LAMPORTS_PER_SOL,
//        withdrawer: Some("5Ryf6H4VeJjPwWMuM42ZoT2JgWr91zFUSRp6Vfqp9sV3"),
//    },
];

//pub const GRANTS_STAKER_INFOS: &[StakerInfo] = &[
//    StakerInfo {
//        name: "rightful agreement",
//        staker: "8w5cgUQfXAZZWyVgenPHpQ1uABXUVLnymqXbuZPx7yqt",
//        lamports: 5_000_000 * LAMPORTS_PER_SOL,
//        withdrawer: Some("EDwSQShtUWQtmFfN9SpUUd6hgonL7tRdxngAsNKv9Pe6"),
//    },
//];
//
//pub const COMMUNITY_STAKER_INFOS: &[StakerInfo] = &[
//    StakerInfo {
//        name: "shrill charity",
//        staker: "Eo1iDtrZZiAkQFA8u431hedChaSUnPbU8MWg849MFvEZ",
//        lamports: 5_000_000 * LAMPORTS_PER_SOL,
//        withdrawer: Some("8CUUMKYNGxdgYio5CLHRHyzMEhhVRMcqefgE6dLqnVRK"),
//    },
//];

fn add_stakes(
    genesis_config: &mut GenesisConfig,
    staker_infos: &[StakerInfo],
    unlock_info: &UnlockInfo,
) -> u64 {
    staker_infos
        .iter()
        .map(|staker_info| create_and_add_stakes(genesis_config, staker_info, unlock_info, None))
        .sum::<u64>()
}

pub fn add_genesis_accounts(genesis_config: &mut GenesisConfig, mut issued_lamports: u64) {
    // add_stakes() and add_validators() award tokens for rent exemption and
    //  to cover an initial transfer-free period of the network

    issued_lamports +=
//        add_stakes(
//        genesis_config,
//        CREATOR_STAKER_INFOS,
//        &UNLOCKS_HALF_AT_9_MONTHS,
//    ) +
            add_stakes(
        genesis_config,
        SERVICE_STAKER_INFOS,
        &UNLOCKS_ALL_AT_9_MONTHS,
    ) + add_stakes(
        genesis_config,
        FOUNDATION_STAKER_INFOS,
        &UNLOCKS_ALL_DAY_ZERO,
    );
//    + add_stakes(genesis_config, GRANTS_STAKER_INFOS, &UNLOCKS_ALL_DAY_ZERO)
//        + add_stakes(
//            genesis_config,
//            COMMUNITY_STAKER_INFOS,
//            &UNLOCKS_ALL_DAY_ZERO,
//        );

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_genesis_accounts() {
        let mut genesis_config = GenesisConfig::default();

        add_genesis_accounts(&mut genesis_config, 0);

        let lamports = genesis_config
            .accounts
            .iter()
            .map(|(_, account)| account.lamports)
            .sum::<u64>();

        assert_eq!(500_000_000 * LAMPORTS_PER_SOL, lamports);
    }
}
