use log::*;
use solana_sdk::{
    account::{AccountSharedData, ReadableAccount, WritableAccount},
    account_utils::StateMut,
    feature_set, ic_msg,
    instruction::InstructionError,
    keyed_account::{from_keyed_account, get_signers, keyed_account_at_index, KeyedAccount},
    native_token::sol_to_lamports,
    nonce,
    nonce_keyed_account::NonceKeyedAccount,
    process_instruction::InvokeContext,
    process_instruction::get_sysvar,
    program_utils::limited_deserialize,
    pubkey::Pubkey,
    system_instruction::{SystemError, SystemInstruction, MAX_PERMITTED_DATA_LENGTH},
    system_program,
    sysvar::{self, recent_blockhashes::RecentBlockhashes, rent::Rent},
};
use solana_sdk::{fnode_data::*,grant_data::*};
use std::collections::HashSet;
use solana_sdk::hash::{Hash, hash};
use bincode::serialize;

// represents an address that may or may not have been generated
//  from a seed
#[derive(PartialEq, Default, Debug)]
struct Address {
    address: Pubkey,
    base: Option<Pubkey>,
}

impl Address {
    fn is_signer(&self, signers: &HashSet<Pubkey>) -> bool {
        if let Some(base) = self.base {
            signers.contains(&base)
        } else {
            signers.contains(&self.address)
        }
    }
    fn create(
        address: &Pubkey,
        with_seed: Option<(&Pubkey, &str, &Pubkey)>,
        invoke_context: &dyn InvokeContext,
    ) -> Result<Self, InstructionError> {
        let base = if let Some((base, seed, owner)) = with_seed {
            let address_with_seed = Pubkey::create_with_seed(base, seed, owner)?;
            // re-derive the address, must match the supplied address
            if *address != address_with_seed {
                ic_msg!(
                    invoke_context,
                    "Create: address {} does not match derived address {}",
                    address,
                    address_with_seed
                );
                return Err(SystemError::AddressWithSeedMismatch.into());
            }
            Some(*base)
        } else {
            None
        };

        Ok(Self {
            address: *address,
            base,
        })
    }
}

fn allocate(
    account: &mut AccountSharedData,
    address: &Address,
    space: u64,
    signers: &HashSet<Pubkey>,
    invoke_context: &dyn InvokeContext,
) -> Result<(), InstructionError> {
    if !address.is_signer(signers) {
        ic_msg!(
            invoke_context,
            "Allocate: 'to' account {:?} must sign",
            address
        );
        return Err(InstructionError::MissingRequiredSignature);
    }

    // if it looks like the `to` account is already in use, bail
    //   (note that the id check is also enforced by message_processor)
    if !account.data().is_empty() || !system_program::check_id(account.owner()) {
        ic_msg!(
            invoke_context,
            "Allocate: account {:?} already in use",
            address
        );
        return Err(SystemError::AccountAlreadyInUse.into());
    }

    if space > MAX_PERMITTED_DATA_LENGTH {
        ic_msg!(
            invoke_context,
            "Allocate: requested {}, max allowed {}",
            space,
            MAX_PERMITTED_DATA_LENGTH
        );
        return Err(SystemError::InvalidAccountDataLength.into());
    }

    account.set_data(vec![0; space as usize]);

    Ok(())
}

fn assign(
    account: &mut AccountSharedData,
    address: &Address,
    owner: &Pubkey,
    signers: &HashSet<Pubkey>,
    invoke_context: &dyn InvokeContext,
) -> Result<(), InstructionError> {
    // no work to do, just return
    if account.owner() == owner {
        return Ok(());
    }

    if !address.is_signer(signers) {
        ic_msg!(invoke_context, "Assign: account {:?} must sign", address);
        return Err(InstructionError::MissingRequiredSignature);
    }

    // bpf programs are allowed to do this; so this is inconsistent...
    // Thus, we're starting to remove this restriction from system instruction
    // processor for consistency and fewer special casing by piggybacking onto
    // the related feature gate..
    let rent_for_sysvars = invoke_context.is_feature_active(&feature_set::rent_for_sysvars::id());
    if !rent_for_sysvars && sysvar::check_id(owner) {
        // guard against sysvars being made
        ic_msg!(invoke_context, "Assign: cannot assign to sysvar, {}", owner);
        return Err(SystemError::InvalidProgramId.into());
    }

    account.set_owner(*owner);
    Ok(())
}

fn allocate_and_assign(
    to: &mut AccountSharedData,
    to_address: &Address,
    space: u64,
    owner: &Pubkey,
    signers: &HashSet<Pubkey>,
    invoke_context: &dyn InvokeContext,
) -> Result<(), InstructionError> {
    allocate(to, to_address, space, signers, invoke_context)?;
    assign(to, to_address, owner, signers, invoke_context)
}

fn create_account(
    from: &KeyedAccount,
    to: &KeyedAccount,
    to_address: &Address,
    lamports: u64,
    space: u64,
    owner: &Pubkey,
    signers: &HashSet<Pubkey>,
    invoke_context: &dyn InvokeContext,
) -> Result<(), InstructionError> {
    // if it looks like the `to` account is already in use, bail
    {
        let to = &mut to.try_account_ref_mut()?;
        if to.lamports() > 0 {
            ic_msg!(
                invoke_context,
                "Create Account: account {:?} already in use",
                to_address
            );
            return Err(SystemError::AccountAlreadyInUse.into());
        }

        allocate_and_assign(to, to_address, space, owner, signers, invoke_context)?;
    }
    transfer(from, to, lamports, invoke_context)
}

fn transfer_verified(
    from: &KeyedAccount,
    to: &KeyedAccount,
    lamports: u64,
    invoke_context: &dyn InvokeContext,
) -> Result<(), InstructionError> {
    if !from.data_is_empty()? {
        ic_msg!(invoke_context, "Transfer: `from` must not carry data");
        return Err(InstructionError::InvalidArgument);
    }
    if lamports > from.lamports()? {
        ic_msg!(
            invoke_context,
            "Transfer: insufficient lamports {}, need {}",
            from.lamports()?,
            lamports
        );
        return Err(SystemError::ResultWithNegativeLamports.into());
    }

    from.try_account_ref_mut()?.checked_sub_lamports(lamports)?;
    to.try_account_ref_mut()?.checked_add_lamports(lamports)?;
    Ok(())
}

fn createfnode(
    from: &KeyedAccount,
    fnode_account: &KeyedAccount,
    reward_address: &Pubkey,
    node_type: i8,
    invoke_context: &dyn InvokeContext,
) -> Result<(), InstructionError> {
    // todo : lamports according to node_type
    let lamports:u64;
    if node_type == 0
    {lamports = sol_to_lamports(COLLATERAL_PHOENIX);}
    else if node_type == 1
    {lamports = sol_to_lamports(COLLATERAL_NOUA);}
    else if node_type == 2
    {lamports = sol_to_lamports(COLLATERAL_FULGUR);}
    else
    {
        return Err(InstructionError::InvalidInstructionData);
    }

    if !invoke_context.is_feature_active(&feature_set::system_transfer_zero_check::id())
        && lamports == 0
    {
        return Ok(());
    }

    // checks to make sure data is written to SysvarFnodeData
    if fnode_account.unsigned_key() != &sysvar::fnode_data::id()
    {
        ic_msg!(
            invoke_context,
            "CreateFNode: account {} is not SysvarFNodeData",
            fnode_account.unsigned_key()
        );
        return Err(InstructionError::InvalidInstructionData);
    }
    if from.signer_key().is_none() {
        ic_msg!(
            invoke_context,
            "Transfer: `from` account {} must sign",
            from.unsigned_key()
        );
        return Err(InstructionError::MissingRequiredSignature);
    }
    // todo: check return value of burn_verified
    // todo: populate sysvarNodeData after burn_verified with node info
    // burn_verified(from, lamports, invoke_context);
    if Ok(()) == transfer_verified(from, fnode_account, lamports, invoke_context)
    {
        let new_fnode: NodeData = (reward_address.clone(), node_type, 0 as u64, false);

// Read Sysvar data
        let mut fnode_data: FNodeData = Some(get_sysvar::<FNodeData>(invoke_context, &sysvar::fnode_data::id())?).unwrap();
        fnode_data.add(new_fnode); // add new node to previous data
        let fnode_data_vec = fnode_data.clone(); // clone
// serialize data
        let mut data: Vec<u8> = Vec::with_capacity(fnode_data_vec.len());
        bincode::serialize_into(&mut data, &fnode_data).map_err(|err| {
            ic_msg!(invoke_context, "Unable to serialize sysvar: {:?}", err);
            InstructionError::GenericError
        })?;
// mut account and store state
        let mut account = fnode_account.try_account_ref_mut()?;
        account.set_data(data);
        Ok(())
    }
    else {
        ic_msg!(
            invoke_context,
            "CreateFNode: burn failed"
        );
        return Err(InstructionError::InvalidInstructionData);
    }
}

fn add_grant(
    from: &KeyedAccount,
    grant_account: &KeyedAccount,
    id: i16,
    receiving_address: &Pubkey,
    amount: u64,
    invoke_context: &dyn InvokeContext,
) -> Result<(), InstructionError> {
    // checks to make sure data is written to GrantData
    if grant_account.unsigned_key() != &sysvar::grant_data::id()
    {
        ic_msg!(
            invoke_context,
            "AddGrant: account {} is not GrantData",
            grant_account.unsigned_key()
        );
        return Err(InstructionError::InvalidInstructionData);
    }
    if from.signer_key().is_none() {
        ic_msg!(
            invoke_context,
            "AddGrant: `from` account {} must sign",
            from.unsigned_key()
        );
        return Err(InstructionError::MissingRequiredSignature);
    }
    //todo : add auth_pubkey
    if from.unsigned_key() != from.unsigned_key() //auth_pubkey_to_add_grant
    {
        ic_msg!(
            invoke_context,
            "AddGrant: {} not authorised_pubkey ",
            from.unsigned_key()
        );
        return Err(InstructionError::MissingRequiredSignature);
    }

    //let vec_votes :  Vec<Pubkey> = Vec::new();   //depr by new vec_votes....
    // todo calc hash of <ID,recv_addr,amount>... remove this frji data
    let id_bytes = serialize(&id).unwrap();
    let recv_address_bytes = serialize(&receiving_address).unwrap();
    let amount_bytes = serialize(&amount).unwrap();
//    let mut vec_bytes: Vec<u8> = Vec::with_capacity(id_bytes.len()+recv_address_bytes.len()+amount_bytes.len());
    let mut vec_bytes: Vec<u8> = Vec::new();
    vec_bytes.extend(id_bytes);
    vec_bytes.extend(recv_address_bytes);
    vec_bytes.extend(amount_bytes);
    let grant_hash = hash(&vec_bytes);

    let vec_votes = vec![Pubkey::default()];

    let new_grant: GrantData = (grant_hash, id, *receiving_address, amount, 0 as i32, vec_votes);

// Read Sysvar data
    let mut grant_data: VecGrantData = Some(get_sysvar::<VecGrantData>(invoke_context, &sysvar::grant_data::id())?).unwrap();
    let mut grant_data_vec = grant_data.clone(); // clone
    match grant_data_vec.binary_search_by(|(hash, _, _, _, _, _)| grant_hash.cmp(hash)) {
        Ok(index) => return Err(InstructionError::MissingRequiredSignature),
        Err(index) => grant_data.add(new_grant),
    }
    grant_data_vec = grant_data.clone();
// serialize data
    let mut data: Vec<u8> = Vec::with_capacity(grant_data_vec.len());
    bincode::serialize_into(&mut data, &grant_data).map_err(|err| {
        ic_msg!(invoke_context, "Unable to serialize sysvar: {:?}", err);
        InstructionError::GenericError
    })?;
// mut account and store state
    let mut account = grant_account.try_account_ref_mut()?;
    account.set_data(data);

    Ok(())
}

// Vote comes from reward_address of FNode, only 0,1 Nodetype can vote
// node_type:0 has 5 votes while node_type:1 has 1 vote
// Make sure grant_hash exists in GrantNode && from.unsignedkey is present in FNodeData
// todo : make sure a node cannot vote twice
// count number of nodes with same reward_address_pubkey as from.pubkey
// count number of votes of this node from vec_votes from grant_data
// node_count >= vote_count from that rewrd_addr_pubkey
// add pubkey to votes and store back to grant_data

fn vote_on_grant(
    from: &KeyedAccount,
    grant_account: &KeyedAccount,
    grant_hash: Hash,
    vote: bool,
    invoke_context: &dyn InvokeContext,
) -> Result<(), InstructionError> {
    // checks to make sure data is written to GrantData
    if grant_account.unsigned_key() != &sysvar::grant_data::id()
    {
        ic_msg!(
            invoke_context,
            "Vote: account {} is not GrantData",
            grant_account.unsigned_key()
        );
        return Err(InstructionError::InvalidInstructionData);
    }
    if from.signer_key().is_none() {
        ic_msg!(
            invoke_context,
            "Vote: `from` account {} must sign",
            from.unsigned_key()
        );
        return Err(InstructionError::MissingRequiredSignature);
    }


    //Find which node is trying to vote return Err if Node is not present
    let mut fnode_data: FNodeData = Some(get_sysvar::<FNodeData>(invoke_context, &sysvar::fnode_data::id())?).unwrap();
    let fnode_data_vec = fnode_data.clone(); // clone to get vector
    let mut node_count = 0;
    let mut vote_count_of_node = 0;
    let mut vec_votes :  Vec<Pubkey> = Vec::new();
    let mut vec_node_types_same_pubkey : Vec<i8> = Vec::new();

    for fnode in fnode_data_vec.iter()
    {
        if fnode.3 == true { //count only active nodes
            if fnode.0 == *from.unsigned_key() {
                node_count += 1;
                vec_node_types_same_pubkey.push(fnode.1);
            }
        }
    }
    // todo: remove zero pubkey from vec_votes when first vote comes
    vec_node_types_same_pubkey.sort();
    if node_count==0 { return Err(InstructionError::InvalidInstructionData); }
    let mut grant_data: VecGrantData = Some(get_sysvar::<VecGrantData>(invoke_context, &sysvar::grant_data::id())?).unwrap();
    let mut grant_data_vec = grant_data.clone(); // clone to get vector
    let mut grant_index = 0;
    match grant_data_vec.binary_search_by(|(hash, _, _, _, _, _)| grant_hash.cmp(hash)) {
        Ok(index) => {vec_votes = grant_data_vec[index].5.clone(); grant_index = index},
        Err(index) => return Err(InstructionError::InvalidInstructionData),
    }

    for vote in vec_votes.iter(){
        if vote == from.unsigned_key() {vote_count_of_node += 1}
    }
    if node_count > vote_count_of_node {
        // insert frompubkey to Votes and update Vote_Weight according to node_type
        //state update
        vec_votes.push(*from.unsigned_key()); //insert to votes
        if grant_index!=0 {
            grant_data_vec[grant_index].4 += {
                match vec_node_types_same_pubkey[vote_count_of_node] {
                    0 => if vote==true {13} else{-13},
                    1 => if vote==true {3} else{-3},
                    2 => if vote==true {1} else{-1},
                    _ =>  0,
                }
            };//vote_weight
            grant_data_vec[grant_index].5 = vec_votes;//change pushed votes
        }
// serialize data
        let mut data: Vec<u8> = Vec::with_capacity(grant_data_vec.len());
        grant_data.replace_with(grant_data_vec);
        bincode::serialize_into(&mut data, &grant_data).map_err(|err| {
            ic_msg!(invoke_context, "Unable to serialize sysvar: {:?}", err);
            InstructionError::GenericError
        })?;
// mut account and store state
        let mut account = grant_account.try_account_ref_mut()?;
        account.set_data(data);

        Ok(())
    }
    else {
        return Err(InstructionError::InvalidInstructionData);
    }
}


fn transfer(
    from: &KeyedAccount,
    to: &KeyedAccount,
    lamports: u64,
    invoke_context: &dyn InvokeContext,
) -> Result<(), InstructionError> {
    if !invoke_context.is_feature_active(&feature_set::system_transfer_zero_check::id())
        && lamports == 0
    {
        return Ok(());
    }

    if from.signer_key().is_none() {
        ic_msg!(
            invoke_context,
            "Transfer: `from` account {} must sign",
            from.unsigned_key()
        );
        return Err(InstructionError::MissingRequiredSignature);
    }

    transfer_verified(from, to, lamports, invoke_context)
}

fn transfer_with_seed(
    from: &KeyedAccount,
    from_base: &KeyedAccount,
    from_seed: &str,
    from_owner: &Pubkey,
    to: &KeyedAccount,
    lamports: u64,
    invoke_context: &dyn InvokeContext,
) -> Result<(), InstructionError> {
    if !invoke_context.is_feature_active(&feature_set::system_transfer_zero_check::id())
        && lamports == 0
    {
        return Ok(());
    }

    if from_base.signer_key().is_none() {
        ic_msg!(
            invoke_context,
            "Transfer: 'from' account {:?} must sign",
            from_base
        );
        return Err(InstructionError::MissingRequiredSignature);
    }

    let address_from_seed =
        Pubkey::create_with_seed(from_base.unsigned_key(), from_seed, from_owner)?;
    if *from.unsigned_key() != address_from_seed {
        ic_msg!(
            invoke_context,
            "Transfer: 'from' address {} does not match derived address {}",
            from.unsigned_key(),
            address_from_seed
        );
        return Err(SystemError::AddressWithSeedMismatch.into());
    }

    transfer_verified(from, to, lamports, invoke_context)
}

pub fn process_instruction(
    _owner: &Pubkey,
    instruction_data: &[u8],
    invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    let keyed_accounts = invoke_context.get_keyed_accounts()?;
    let instruction = limited_deserialize(instruction_data)?;

    trace!("process_instruction: {:?}", instruction);
    trace!("keyed_accounts: {:?}", keyed_accounts);

    let signers = get_signers(keyed_accounts);

    match instruction {
        SystemInstruction::CreateAccount {
            lamports,
            space,
            owner,
        } => {
            let from = keyed_account_at_index(keyed_accounts, 0)?;
            let to = keyed_account_at_index(keyed_accounts, 1)?;
            let to_address = Address::create(to.unsigned_key(), None, invoke_context)?;
            create_account(
                from,
                to,
                &to_address,
                lamports,
                space,
                &owner,
                &signers,
                invoke_context,
            )
        }
        SystemInstruction::CreateAccountWithSeed {
            base,
            seed,
            lamports,
            space,
            owner,
        } => {
            let from = keyed_account_at_index(keyed_accounts, 0)?;
            let to = keyed_account_at_index(keyed_accounts, 1)?;
            let to_address = Address::create(
                to.unsigned_key(),
                Some((&base, &seed, &owner)),
                invoke_context,
            )?;
            create_account(
                from,
                to,
                &to_address,
                lamports,
                space,
                &owner,
                &signers,
                invoke_context,
            )
        }
        SystemInstruction::Assign { owner } => {
            let keyed_account = keyed_account_at_index(keyed_accounts, 0)?;
            let mut account = keyed_account.try_account_ref_mut()?;
            let address = Address::create(keyed_account.unsigned_key(), None, invoke_context)?;
            assign(&mut account, &address, &owner, &signers, invoke_context)
        }
        SystemInstruction::Transfer { lamports } => {
            let from = keyed_account_at_index(keyed_accounts, 0)?;
            let to = keyed_account_at_index(keyed_accounts, 1)?;
            transfer(from, to, lamports, invoke_context)
        }
        SystemInstruction::TransferWithSeed {
            lamports,
            from_seed,
            from_owner,
        } => {
            let from = keyed_account_at_index(keyed_accounts, 0)?;
            let base = keyed_account_at_index(keyed_accounts, 1)?;
            let to = keyed_account_at_index(keyed_accounts, 2)?;
            transfer_with_seed(
                from,
                base,
                &from_seed,
                &from_owner,
                to,
                lamports,
                invoke_context,
            )
        }
        SystemInstruction::AdvanceNonceAccount => {
            let me = &mut keyed_account_at_index(keyed_accounts, 0)?;
            me.advance_nonce_account(
                &from_keyed_account::<RecentBlockhashes>(keyed_account_at_index(
                    keyed_accounts,
                    1,
                )?)?,
                &signers,
                invoke_context,
            )
        }
        SystemInstruction::WithdrawNonceAccount(lamports) => {
            let me = &mut keyed_account_at_index(keyed_accounts, 0)?;
            let to = &mut keyed_account_at_index(keyed_accounts, 1)?;
            me.withdraw_nonce_account(
                lamports,
                to,
                &from_keyed_account::<RecentBlockhashes>(keyed_account_at_index(
                    keyed_accounts,
                    2,
                )?)?,
                &from_keyed_account::<Rent>(keyed_account_at_index(keyed_accounts, 3)?)?,
                &signers,
                invoke_context,
            )
        }
        SystemInstruction::InitializeNonceAccount(authorized) => {
            let me = &mut keyed_account_at_index(keyed_accounts, 0)?;
            me.initialize_nonce_account(
                &authorized,
                &from_keyed_account::<RecentBlockhashes>(keyed_account_at_index(
                    keyed_accounts,
                    1,
                )?)?,
                &from_keyed_account::<Rent>(keyed_account_at_index(keyed_accounts, 2)?)?,
                invoke_context,
            )
        }
        SystemInstruction::AuthorizeNonceAccount(nonce_authority) => {
            let me = &mut keyed_account_at_index(keyed_accounts, 0)?;
            me.authorize_nonce_account(&nonce_authority, &signers, invoke_context)
        }
        SystemInstruction::Allocate { space } => {
            let keyed_account = keyed_account_at_index(keyed_accounts, 0)?;
            let mut account = keyed_account.try_account_ref_mut()?;
            let address = Address::create(keyed_account.unsigned_key(), None, invoke_context)?;
            allocate(&mut account, &address, space, &signers, invoke_context)
        }
        SystemInstruction::AllocateWithSeed {
            base,
            seed,
            space,
            owner,
        } => {
            let keyed_account = keyed_account_at_index(keyed_accounts, 0)?;
            let mut account = keyed_account.try_account_ref_mut()?;
            let address = Address::create(
                keyed_account.unsigned_key(),
                Some((&base, &seed, &owner)),
                invoke_context,
            )?;
            allocate_and_assign(
                &mut account,
                &address,
                space,
                &owner,
                &signers,
                invoke_context,
            )
        }
        SystemInstruction::AssignWithSeed { base, seed, owner } => {
            let keyed_account = keyed_account_at_index(keyed_accounts, 0)?;
            let mut account = keyed_account.try_account_ref_mut()?;
            let address = Address::create(
                keyed_account.unsigned_key(),
                Some((&base, &seed, &owner)),
                invoke_context,
            )?;
            assign(&mut account, &address, &owner, &signers, invoke_context)
        }
        SystemInstruction::CreateFNode {
            reward_address,
            node_type,
        } => {
            let from = keyed_account_at_index(keyed_accounts, 0)?;
            let fnode_account = keyed_account_at_index(keyed_accounts, 1)?;
            createfnode(from, fnode_account, &reward_address, node_type, invoke_context)
        }
        SystemInstruction::AddGrant { id,
            receiving_address,
            amount,
        } => {
            let from = keyed_account_at_index(keyed_accounts, 0)?;
            let grant_account = keyed_account_at_index(keyed_accounts, 1)?;
            add_grant(from, &grant_account, id, &receiving_address, amount, invoke_context)
        }
        SystemInstruction::VoteOnGrant { grant_hash,
            vote,
        } => {
            let from = keyed_account_at_index(keyed_accounts, 0)?;
            let grant_account = keyed_account_at_index(keyed_accounts, 1)?;
            vote_on_grant(from, &grant_account, grant_hash, vote, invoke_context)
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SystemAccountKind {
    System,
    Nonce,
}

pub fn get_system_account_kind(account: &AccountSharedData) -> Option<SystemAccountKind> {
    if system_program::check_id(account.owner()) {
        if account.data().is_empty() {
            Some(SystemAccountKind::System)
        } else if account.data().len() == nonce::State::size() {
            match account.state().ok()? {
                nonce::state::Versions::Current(state) => match *state {
                    nonce::State::Initialized(_) => Some(SystemAccountKind::Nonce),
                    _ => None,
                },
            }
        } else {
            None
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bank::Bank, bank_client::BankClient};
    use bincode::serialize;
    use solana_sdk::{
        account::{self, Account, AccountSharedData},
        client::SyncClient,
        fee_calculator::FeeCalculator,
        genesis_config::create_genesis_config,
        hash::{hash, Hash},
        instruction::{AccountMeta, Instruction, InstructionError},
        message::Message,
        nonce, nonce_account,
        process_instruction::MockInvokeContext,
        recent_blockhashes_account,
        signature::{Keypair, Signer},
        system_instruction, system_program, sysvar,
        sysvar::recent_blockhashes::IterItem,
        transaction::TransactionError,
    };
    use std::cell::RefCell;
    use std::sync::Arc;

    impl From<Pubkey> for Address {
        fn from(address: Pubkey) -> Self {
            Self {
                address,
                base: None,
            }
        }
    }

    fn process_instruction(
        owner: &Pubkey,
        keyed_accounts: Vec<KeyedAccount>,
        instruction_data: &[u8],
    ) -> Result<(), InstructionError> {
        super::process_instruction(
            owner,
            instruction_data,
            &mut MockInvokeContext::new(keyed_accounts),
        )
    }

    fn create_default_account() -> RefCell<AccountSharedData> {
        RefCell::new(AccountSharedData::default())
    }
    fn create_default_recent_blockhashes_account() -> RefCell<AccountSharedData> {
        RefCell::new(
            recent_blockhashes_account::create_account_with_data_for_test(
                vec![
                    IterItem(0u64, &Hash::default(), &FeeCalculator::default());
                    sysvar::recent_blockhashes::MAX_ENTRIES
                ]
                .into_iter(),
            ),
        )
    }
    fn create_default_rent_account() -> RefCell<AccountSharedData> {
        RefCell::new(account::create_account_shared_data_for_test(&Rent::free()))
    }

    #[test]
    fn test_create_account() {
        let new_owner = Pubkey::new(&[9; 32]);
        let from = solana_sdk::pubkey::new_rand();
        let to = solana_sdk::pubkey::new_rand();
        let from_account = AccountSharedData::new_ref(100, 0, &system_program::id());
        let to_account = AccountSharedData::new_ref(0, 0, &Pubkey::default());

        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![
                    KeyedAccount::new(&from, true, &from_account),
                    KeyedAccount::new(&to, true, &to_account)
                ],
                &bincode::serialize(&SystemInstruction::CreateAccount {
                    lamports: 50,
                    space: 2,
                    owner: new_owner
                })
                .unwrap()
            ),
            Ok(())
        );
        assert_eq!(from_account.borrow().lamports(), 50);
        assert_eq!(to_account.borrow().lamports(), 50);
        assert_eq!(to_account.borrow().owner(), &new_owner);
        assert_eq!(to_account.borrow().data(), &[0, 0]);
    }

    #[test]
    fn test_create_account_with_seed() {
        let new_owner = Pubkey::new(&[9; 32]);
        let from = solana_sdk::pubkey::new_rand();
        let seed = "shiny pepper";
        let to = Pubkey::create_with_seed(&from, seed, &new_owner).unwrap();

        let from_account = AccountSharedData::new_ref(100, 0, &system_program::id());
        let to_account = AccountSharedData::new_ref(0, 0, &Pubkey::default());

        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![
                    KeyedAccount::new(&from, true, &from_account),
                    KeyedAccount::new(&to, false, &to_account)
                ],
                &bincode::serialize(&SystemInstruction::CreateAccountWithSeed {
                    base: from,
                    seed: seed.to_string(),
                    lamports: 50,
                    space: 2,
                    owner: new_owner
                })
                .unwrap()
            ),
            Ok(())
        );
        assert_eq!(from_account.borrow().lamports(), 50);
        assert_eq!(to_account.borrow().lamports(), 50);
        assert_eq!(to_account.borrow().owner(), &new_owner);
        assert_eq!(to_account.borrow().data(), &[0, 0]);
    }

    #[test]
    fn test_create_account_with_seed_separate_base_account() {
        let new_owner = Pubkey::new(&[9; 32]);
        let from = solana_sdk::pubkey::new_rand();
        let base = solana_sdk::pubkey::new_rand();
        let seed = "shiny pepper";
        let to = Pubkey::create_with_seed(&base, seed, &new_owner).unwrap();

        let from_account = AccountSharedData::new_ref(100, 0, &system_program::id());
        let to_account = AccountSharedData::new_ref(0, 0, &Pubkey::default());
        let base_account = AccountSharedData::new_ref(0, 0, &Pubkey::default());

        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![
                    KeyedAccount::new(&from, true, &from_account),
                    KeyedAccount::new(&to, false, &to_account),
                    KeyedAccount::new(&base, true, &base_account)
                ],
                &bincode::serialize(&SystemInstruction::CreateAccountWithSeed {
                    base,
                    seed: seed.to_string(),
                    lamports: 50,
                    space: 2,
                    owner: new_owner
                })
                .unwrap()
            ),
            Ok(())
        );
        assert_eq!(from_account.borrow().lamports(), 50);
        assert_eq!(to_account.borrow().lamports(), 50);
        assert_eq!(to_account.borrow().owner(), &new_owner);
        assert_eq!(to_account.borrow().data(), &[0, 0]);
    }

    #[test]
    fn test_address_create_with_seed_mismatch() {
        let from = solana_sdk::pubkey::new_rand();
        let seed = "dull boy";
        let to = solana_sdk::pubkey::new_rand();
        let owner = solana_sdk::pubkey::new_rand();

        assert_eq!(
            Address::create(
                &to,
                Some((&from, seed, &owner)),
                &MockInvokeContext::new(vec![]),
            ),
            Err(SystemError::AddressWithSeedMismatch.into())
        );
    }

    #[test]
    fn test_create_account_with_seed_missing_sig() {
        let new_owner = Pubkey::new(&[9; 32]);
        let from = solana_sdk::pubkey::new_rand();
        let seed = "dull boy";
        let to = Pubkey::create_with_seed(&from, seed, &new_owner).unwrap();

        let from_account = AccountSharedData::new_ref(100, 0, &system_program::id());
        let to_account = AccountSharedData::new_ref(0, 0, &Pubkey::default());
        let to_address = Address::create(
            &to,
            Some((&from, seed, &new_owner)),
            &MockInvokeContext::new(vec![]),
        )
        .unwrap();

        assert_eq!(
            create_account(
                &KeyedAccount::new(&from, false, &from_account),
                &KeyedAccount::new(&to, false, &to_account),
                &to_address,
                50,
                2,
                &new_owner,
                &HashSet::new(),
                &MockInvokeContext::new(vec![]),
            ),
            Err(InstructionError::MissingRequiredSignature)
        );
        assert_eq!(from_account.borrow().lamports(), 100);
        assert_eq!(*to_account.borrow(), AccountSharedData::default());
    }

    #[test]
    fn test_create_with_zero_lamports() {
        // create account with zero lamports transferred
        let new_owner = Pubkey::new(&[9; 32]);
        let from = Pubkey::new_unique();
        let from_account = AccountSharedData::new_ref(100, 0, &Pubkey::new_unique()); // not from system account

        let to = Pubkey::new_unique();
        let to_account = AccountSharedData::new_ref(0, 0, &Pubkey::default());

        assert_eq!(
            create_account(
                &KeyedAccount::new(&from, true, &from_account),
                &KeyedAccount::new(&to, true, &to_account),
                &to.into(),
                0,
                2,
                &new_owner,
                &[from, to].iter().cloned().collect::<HashSet<_>>(),
                &MockInvokeContext::new(vec![]),
            ),
            Ok(())
        );

        let from_lamports = from_account.borrow().lamports();
        let to_lamports = to_account.borrow().lamports();
        let to_owner = *to_account.borrow().owner();
        assert_eq!(from_lamports, 100);
        assert_eq!(to_lamports, 0);
        assert_eq!(to_owner, new_owner);
        assert_eq!(to_account.borrow().data(), &[0, 0]);
    }

    #[test]
    fn test_create_negative_lamports() {
        // Attempt to create account with more lamports than remaining in from_account
        let new_owner = Pubkey::new(&[9; 32]);
        let from = solana_sdk::pubkey::new_rand();
        let from_account = AccountSharedData::new_ref(100, 0, &system_program::id());

        let to = solana_sdk::pubkey::new_rand();
        let to_account = AccountSharedData::new_ref(0, 0, &Pubkey::default());

        let result = create_account(
            &KeyedAccount::new(&from, true, &from_account),
            &KeyedAccount::new(&from, false, &to_account),
            &to.into(),
            150,
            2,
            &new_owner,
            &[from, to].iter().cloned().collect::<HashSet<_>>(),
            &MockInvokeContext::new(vec![]),
        );
        assert_eq!(result, Err(SystemError::ResultWithNegativeLamports.into()));
    }

    #[test]
    fn test_request_more_than_allowed_data_length() {
        let from_account = AccountSharedData::new_ref(100, 0, &system_program::id());
        let from = solana_sdk::pubkey::new_rand();
        let to_account = AccountSharedData::new_ref(0, 0, &system_program::id());
        let to = solana_sdk::pubkey::new_rand();

        let signers = &[from, to].iter().cloned().collect::<HashSet<_>>();
        let address = &to.into();

        // Trying to request more data length than permitted will result in failure
        let result = create_account(
            &KeyedAccount::new(&from, true, &from_account),
            &KeyedAccount::new(&to, false, &to_account),
            address,
            50,
            MAX_PERMITTED_DATA_LENGTH + 1,
            &system_program::id(),
            signers,
            &MockInvokeContext::new(vec![]),
        );
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap(),
            SystemError::InvalidAccountDataLength.into()
        );

        // Trying to request equal or less data length than permitted will be successful
        let result = create_account(
            &KeyedAccount::new(&from, true, &from_account),
            &KeyedAccount::new(&to, false, &to_account),
            address,
            50,
            MAX_PERMITTED_DATA_LENGTH,
            &system_program::id(),
            signers,
            &MockInvokeContext::new(vec![]),
        );
        assert!(result.is_ok());
        assert_eq!(to_account.borrow().lamports(), 50);
        assert_eq!(
            to_account.borrow().data().len() as u64,
            MAX_PERMITTED_DATA_LENGTH
        );
    }

    #[test]
    fn test_create_already_in_use() {
        // Attempt to create system account in account already owned by another program
        let new_owner = Pubkey::new(&[9; 32]);
        let from = solana_sdk::pubkey::new_rand();
        let from_account = AccountSharedData::new_ref(100, 0, &system_program::id());

        let original_program_owner = Pubkey::new(&[5; 32]);
        let owned_key = solana_sdk::pubkey::new_rand();
        let owned_account = AccountSharedData::new_ref(0, 0, &original_program_owner);
        let unchanged_account = owned_account.clone();

        let signers = &[from, owned_key].iter().cloned().collect::<HashSet<_>>();
        let owned_address = owned_key.into();

        let result = create_account(
            &KeyedAccount::new(&from, true, &from_account),
            &KeyedAccount::new(&owned_key, false, &owned_account),
            &owned_address,
            50,
            2,
            &new_owner,
            signers,
            &MockInvokeContext::new(vec![]),
        );
        assert_eq!(result, Err(SystemError::AccountAlreadyInUse.into()));

        let from_lamports = from_account.borrow().lamports();
        assert_eq!(from_lamports, 100);
        assert_eq!(owned_account, unchanged_account);

        // Attempt to create system account in account that already has data
        let owned_account = AccountSharedData::new_ref(0, 1, &Pubkey::default());
        let unchanged_account = owned_account.borrow().clone();
        let result = create_account(
            &KeyedAccount::new(&from, true, &from_account),
            &KeyedAccount::new(&owned_key, false, &owned_account),
            &owned_address,
            50,
            2,
            &new_owner,
            signers,
            &MockInvokeContext::new(vec![]),
        );
        assert_eq!(result, Err(SystemError::AccountAlreadyInUse.into()));
        let from_lamports = from_account.borrow().lamports();
        assert_eq!(from_lamports, 100);
        assert_eq!(*owned_account.borrow(), unchanged_account);

        // Attempt to create an account that already has lamports
        let owned_account = AccountSharedData::new_ref(1, 0, &Pubkey::default());
        let unchanged_account = owned_account.borrow().clone();
        let result = create_account(
            &KeyedAccount::new(&from, true, &from_account),
            &KeyedAccount::new(&owned_key, false, &owned_account),
            &owned_address,
            50,
            2,
            &new_owner,
            signers,
            &MockInvokeContext::new(vec![]),
        );
        assert_eq!(result, Err(SystemError::AccountAlreadyInUse.into()));
        assert_eq!(from_lamports, 100);
        assert_eq!(*owned_account.borrow(), unchanged_account);
    }

    #[test]
    fn test_create_unsigned() {
        // Attempt to create an account without signing the transfer
        let new_owner = Pubkey::new(&[9; 32]);
        let from = solana_sdk::pubkey::new_rand();
        let from_account = AccountSharedData::new_ref(100, 0, &system_program::id());

        let owned_key = solana_sdk::pubkey::new_rand();
        let owned_account = AccountSharedData::new_ref(0, 0, &Pubkey::default());

        let owned_address = owned_key.into();

        // Haven't signed from account
        let result = create_account(
            &KeyedAccount::new(&from, false, &from_account),
            &KeyedAccount::new(&owned_key, false, &owned_account),
            &owned_address,
            50,
            2,
            &new_owner,
            &[owned_key].iter().cloned().collect::<HashSet<_>>(),
            &MockInvokeContext::new(vec![]),
        );
        assert_eq!(result, Err(InstructionError::MissingRequiredSignature));

        // Haven't signed to account
        let owned_account = AccountSharedData::new_ref(0, 0, &Pubkey::default());
        let result = create_account(
            &KeyedAccount::new(&from, true, &from_account),
            &KeyedAccount::new(&owned_key, true, &owned_account),
            &owned_address,
            50,
            2,
            &new_owner,
            &[from].iter().cloned().collect::<HashSet<_>>(),
            &MockInvokeContext::new(vec![]),
        );
        assert_eq!(result, Err(InstructionError::MissingRequiredSignature));

        // Don't support unsigned creation with zero lamports (ephemeral account)
        let owned_account = AccountSharedData::new_ref(0, 0, &Pubkey::default());
        let result = create_account(
            &KeyedAccount::new(&from, false, &from_account),
            &KeyedAccount::new(&owned_key, true, &owned_account),
            &owned_address,
            0,
            2,
            &new_owner,
            &[owned_key].iter().cloned().collect::<HashSet<_>>(),
            &MockInvokeContext::new(vec![]),
        );
        assert_eq!(result, Err(InstructionError::MissingRequiredSignature));
    }

    #[test]
    fn test_create_sysvar_invalid_id_with_feature() {
        // Attempt to create system account in account already owned by another program
        let from = solana_sdk::pubkey::new_rand();
        let from_account = AccountSharedData::new_ref(100, 0, &system_program::id());

        let to = solana_sdk::pubkey::new_rand();
        let to_account = AccountSharedData::new_ref(0, 0, &system_program::id());

        let signers = [from, to].iter().cloned().collect::<HashSet<_>>();
        let to_address = to.into();

        // fail to create a sysvar::id() owned account
        let result = create_account(
            &KeyedAccount::new(&from, true, &from_account),
            &KeyedAccount::new(&to, false, &to_account),
            &to_address,
            50,
            2,
            &sysvar::id(),
            &signers,
            &MockInvokeContext::new(vec![]),
        );
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn test_create_sysvar_invalid_id_without_feature() {
        // Attempt to create system account in account already owned by another program
        let from = solana_sdk::pubkey::new_rand();
        let from_account = AccountSharedData::new_ref(100, 0, &system_program::id());

        let to = solana_sdk::pubkey::new_rand();
        let to_account = AccountSharedData::new_ref(0, 0, &system_program::id());

        let signers = [from, to].iter().cloned().collect::<HashSet<_>>();
        let to_address = to.into();

        let result = create_account(
            &KeyedAccount::new(&from, true, &from_account),
            &KeyedAccount::new(&to, false, &to_account),
            &to_address,
            50,
            2,
            &sysvar::id(),
            &signers,
            &MockInvokeContext {
                disabled_features: vec![feature_set::rent_for_sysvars::id()]
                    .into_iter()
                    .collect(),
                ..MockInvokeContext::new(vec![])
            },
        );
        assert_eq!(result, Err(SystemError::InvalidProgramId.into()));
    }

    #[test]
    fn test_create_data_populated() {
        // Attempt to create system account in account with populated data
        let new_owner = Pubkey::new(&[9; 32]);
        let from = solana_sdk::pubkey::new_rand();
        let from_account = AccountSharedData::new_ref(100, 0, &system_program::id());

        let populated_key = solana_sdk::pubkey::new_rand();
        let populated_account = AccountSharedData::from(Account {
            data: vec![0, 1, 2, 3],
            ..Account::default()
        })
        .into();

        let signers = [from, populated_key]
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let populated_address = populated_key.into();

        let result = create_account(
            &KeyedAccount::new(&from, true, &from_account),
            &KeyedAccount::new(&populated_key, false, &populated_account),
            &populated_address,
            50,
            2,
            &new_owner,
            &signers,
            &MockInvokeContext::new(vec![]),
        );
        assert_eq!(result, Err(SystemError::AccountAlreadyInUse.into()));
    }

    #[test]
    fn test_create_from_account_is_nonce_fail() {
        let nonce = solana_sdk::pubkey::new_rand();
        let nonce_account = AccountSharedData::new_ref_data(
            42,
            &nonce::state::Versions::new_current(nonce::State::Initialized(
                nonce::state::Data::default(),
            )),
            &system_program::id(),
        )
        .unwrap();
        let from = KeyedAccount::new(&nonce, true, &nonce_account);
        let new = solana_sdk::pubkey::new_rand();

        let new_account = AccountSharedData::new_ref(0, 0, &system_program::id());

        let signers = [nonce, new].iter().cloned().collect::<HashSet<_>>();
        let new_address = new.into();
        let new_keyed_account = KeyedAccount::new(&new, false, &new_account);

        assert_eq!(
            create_account(
                &from,
                &new_keyed_account,
                &new_address,
                42,
                0,
                &solana_sdk::pubkey::new_rand(),
                &signers,
                &MockInvokeContext::new(vec![]),
            ),
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_assign() {
        let new_owner = Pubkey::new(&[9; 32]);

        let pubkey = solana_sdk::pubkey::new_rand();
        let mut account = AccountSharedData::new(100, 0, &system_program::id());

        assert_eq!(
            assign(
                &mut account,
                &pubkey.into(),
                &new_owner,
                &HashSet::new(),
                &MockInvokeContext::new(vec![]),
            ),
            Err(InstructionError::MissingRequiredSignature)
        );
        // no change, no signature needed
        assert_eq!(
            assign(
                &mut account,
                &pubkey.into(),
                &system_program::id(),
                &HashSet::new(),
                &MockInvokeContext::new(vec![]),
            ),
            Ok(())
        );

        let account = RefCell::new(account);
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![KeyedAccount::new(&pubkey, true, &account)],
                &bincode::serialize(&SystemInstruction::Assign { owner: new_owner }).unwrap()
            ),
            Ok(())
        );
    }

    #[test]
    fn test_assign_to_sysvar_with_feature() {
        let new_owner = sysvar::id();

        let from = solana_sdk::pubkey::new_rand();
        let mut from_account = AccountSharedData::new(100, 0, &system_program::id());

        assert_eq!(
            assign(
                &mut from_account,
                &from.into(),
                &new_owner,
                &[from].iter().cloned().collect::<HashSet<_>>(),
                &MockInvokeContext::new(vec![]),
            ),
            Ok(())
        );
    }

    #[test]
    fn test_assign_to_sysvar_without_feature() {
        let new_owner = sysvar::id();

        let from = solana_sdk::pubkey::new_rand();
        let mut from_account = AccountSharedData::new(100, 0, &system_program::id());

        assert_eq!(
            assign(
                &mut from_account,
                &from.into(),
                &new_owner,
                &[from].iter().cloned().collect::<HashSet<_>>(),
                &MockInvokeContext {
                    disabled_features: vec![feature_set::rent_for_sysvars::id()]
                        .into_iter()
                        .collect(),
                    ..MockInvokeContext::new(vec![])
                },
            ),
            Err(SystemError::InvalidProgramId.into())
        );
    }

    #[test]
    fn test_process_bogus_instruction() {
        // Attempt to assign with no accounts
        let instruction = SystemInstruction::Assign {
            owner: solana_sdk::pubkey::new_rand(),
        };
        let data = serialize(&instruction).unwrap();
        let result = process_instruction(&system_program::id(), vec![], &data);
        assert_eq!(result, Err(InstructionError::NotEnoughAccountKeys));

        let from = solana_sdk::pubkey::new_rand();
        let from_account = AccountSharedData::new_ref(100, 0, &system_program::id());
        // Attempt to transfer with no destination
        let instruction = SystemInstruction::Transfer { lamports: 0 };
        let data = serialize(&instruction).unwrap();
        let result = process_instruction(
            &system_program::id(),
            vec![KeyedAccount::new(&from, true, &from_account)],
            &data,
        );
        assert_eq!(result, Err(InstructionError::NotEnoughAccountKeys));
    }

    #[test]
    fn test_transfer_lamports() {
        let from = solana_sdk::pubkey::new_rand();
        let from_account = AccountSharedData::new_ref(100, 0, &Pubkey::new(&[2; 32])); // account owner should not matter
        let to = Pubkey::new(&[3; 32]);
        let to_account = AccountSharedData::new_ref(1, 0, &to); // account owner should not matter
        let from_keyed_account = KeyedAccount::new(&from, true, &from_account);
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);
        transfer(
            &from_keyed_account,
            &to_keyed_account,
            50,
            &MockInvokeContext::new(vec![]),
        )
        .unwrap();
        let from_lamports = from_keyed_account.account.borrow().lamports();
        let to_lamports = to_keyed_account.account.borrow().lamports();
        assert_eq!(from_lamports, 50);
        assert_eq!(to_lamports, 51);

        // Attempt to move more lamports than remaining in from_account
        let from_keyed_account = KeyedAccount::new(&from, true, &from_account);
        let result = transfer(
            &from_keyed_account,
            &to_keyed_account,
            100,
            &MockInvokeContext::new(vec![]),
        );
        assert_eq!(result, Err(SystemError::ResultWithNegativeLamports.into()));
        assert_eq!(from_keyed_account.account.borrow().lamports(), 50);
        assert_eq!(to_keyed_account.account.borrow().lamports(), 51);

        // test signed transfer of zero
        assert!(transfer(
            &from_keyed_account,
            &to_keyed_account,
            0,
            &MockInvokeContext::new(vec![]),
        )
        .is_ok(),);
        assert_eq!(from_keyed_account.account.borrow().lamports(), 50);
        assert_eq!(to_keyed_account.account.borrow().lamports(), 51);

        // test unsigned transfer of zero
        let from_keyed_account = KeyedAccount::new(&from, false, &from_account);

        assert_eq!(
            transfer(
                &from_keyed_account,
                &to_keyed_account,
                0,
                &MockInvokeContext::new(vec![]),
            ),
            Err(InstructionError::MissingRequiredSignature)
        );
        assert_eq!(from_keyed_account.account.borrow().lamports(), 50);
        assert_eq!(to_keyed_account.account.borrow().lamports(), 51);
    }

    #[test]
    fn test_transfer_with_seed() {
        let base = solana_sdk::pubkey::new_rand();
        let base_account = AccountSharedData::new_ref(100, 0, &Pubkey::new(&[2; 32])); // account owner should not matter
        let from_base_keyed_account = KeyedAccount::new(&base, true, &base_account);
        let from_seed = "42";
        let from_owner = system_program::id();
        let from = Pubkey::create_with_seed(&base, from_seed, &from_owner).unwrap();
        let from_account = AccountSharedData::new_ref(100, 0, &Pubkey::new(&[2; 32])); // account owner should not matter
        let to = Pubkey::new(&[3; 32]);
        let to_account = AccountSharedData::new_ref(1, 0, &to); // account owner should not matter
        let from_keyed_account = KeyedAccount::new(&from, true, &from_account);
        let to_keyed_account = KeyedAccount::new(&to, false, &to_account);
        transfer_with_seed(
            &from_keyed_account,
            &from_base_keyed_account,
            from_seed,
            &from_owner,
            &to_keyed_account,
            50,
            &MockInvokeContext::new(vec![]),
        )
        .unwrap();
        let from_lamports = from_keyed_account.account.borrow().lamports();
        let to_lamports = to_keyed_account.account.borrow().lamports();
        assert_eq!(from_lamports, 50);
        assert_eq!(to_lamports, 51);

        // Attempt to move more lamports than remaining in from_account
        let from_keyed_account = KeyedAccount::new(&from, true, &from_account);
        let result = transfer_with_seed(
            &from_keyed_account,
            &from_base_keyed_account,
            from_seed,
            &from_owner,
            &to_keyed_account,
            100,
            &MockInvokeContext::new(vec![]),
        );
        assert_eq!(result, Err(SystemError::ResultWithNegativeLamports.into()));
        assert_eq!(from_keyed_account.account.borrow().lamports(), 50);
        assert_eq!(to_keyed_account.account.borrow().lamports(), 51);

        // test unsigned transfer of zero
        let from_keyed_account = KeyedAccount::new(&from, false, &from_account);
        assert!(transfer_with_seed(
            &from_keyed_account,
            &from_base_keyed_account,
            from_seed,
            &from_owner,
            &to_keyed_account,
            0,
            &MockInvokeContext::new(vec![]),
        )
        .is_ok(),);
        assert_eq!(from_keyed_account.account.borrow().lamports(), 50);
        assert_eq!(to_keyed_account.account.borrow().lamports(), 51);
    }

    #[test]
    fn test_transfer_lamports_from_nonce_account_fail() {
        let from = solana_sdk::pubkey::new_rand();
        let from_account = AccountSharedData::new_ref_data(
            100,
            &nonce::state::Versions::new_current(nonce::State::Initialized(nonce::state::Data {
                authority: from,
                ..nonce::state::Data::default()
            })),
            &system_program::id(),
        )
        .unwrap();
        assert_eq!(
            get_system_account_kind(&from_account.borrow()),
            Some(SystemAccountKind::Nonce)
        );

        let to = Pubkey::new(&[3; 32]);
        let to_account = AccountSharedData::new_ref(1, 0, &to); // account owner should not matter
        assert_eq!(
            transfer(
                &KeyedAccount::new(&from, true, &from_account),
                &KeyedAccount::new(&to, false, &to_account),
                50,
                &MockInvokeContext::new(vec![]),
            ),
            Err(InstructionError::InvalidArgument),
        )
    }

    #[test]
    fn test_allocate() {
        let (genesis_config, mint_keypair) = create_genesis_config(100);
        let bank = Bank::new(&genesis_config);
        let bank_client = BankClient::new(bank);

        let alice_keypair = Keypair::new();
        let alice_pubkey = alice_keypair.pubkey();
        let seed = "seed";
        let owner = solana_sdk::pubkey::new_rand();
        let alice_with_seed = Pubkey::create_with_seed(&alice_pubkey, seed, &owner).unwrap();

        bank_client
            .transfer_and_confirm(50, &mint_keypair, &alice_pubkey)
            .unwrap();

        let allocate_with_seed = Message::new(
            &[system_instruction::allocate_with_seed(
                &alice_with_seed,
                &alice_pubkey,
                seed,
                2,
                &owner,
            )],
            Some(&alice_pubkey),
        );

        assert!(bank_client
            .send_and_confirm_message(&[&alice_keypair], allocate_with_seed)
            .is_ok());

        let allocate = system_instruction::allocate(&alice_pubkey, 2);

        assert!(bank_client
            .send_and_confirm_instruction(&alice_keypair, allocate)
            .is_ok());
    }

    fn with_create_zero_lamport<F>(callback: F)
    where
        F: Fn(&Bank),
    {
        solana_logger::setup();

        let alice_keypair = Keypair::new();
        let bob_keypair = Keypair::new();

        let alice_pubkey = alice_keypair.pubkey();
        let bob_pubkey = bob_keypair.pubkey();

        let program = solana_sdk::pubkey::new_rand();
        let collector = solana_sdk::pubkey::new_rand();

        let mint_lamports = 10000;
        let len1 = 123;
        let len2 = 456;

        // create initial bank and fund the alice account
        let (genesis_config, mint_keypair) = create_genesis_config(mint_lamports);
        let bank = Arc::new(Bank::new(&genesis_config));
        let bank_client = BankClient::new_shared(&bank);
        bank_client
            .transfer_and_confirm(mint_lamports, &mint_keypair, &alice_pubkey)
            .unwrap();

        // create zero-lamports account to be cleaned
        let bank = Arc::new(Bank::new_from_parent(&bank, &collector, bank.slot() + 1));
        let bank_client = BankClient::new_shared(&bank);
        let ix = system_instruction::create_account(&alice_pubkey, &bob_pubkey, 0, len1, &program);
        let message = Message::new(&[ix], Some(&alice_keypair.pubkey()));
        let r = bank_client.send_and_confirm_message(&[&alice_keypair, &bob_keypair], message);
        assert!(r.is_ok());

        // transfer some to bogus pubkey just to make previous bank (=slot) really cleanable
        let bank = Arc::new(Bank::new_from_parent(&bank, &collector, bank.slot() + 1));
        let bank_client = BankClient::new_shared(&bank);
        bank_client
            .transfer_and_confirm(50, &alice_keypair, &solana_sdk::pubkey::new_rand())
            .unwrap();

        // super fun time; callback chooses to .clean_accounts(None) or not
        callback(&*bank);

        // create a normal account at the same pubkey as the zero-lamports account
        let bank = Arc::new(Bank::new_from_parent(&bank, &collector, bank.slot() + 1));
        let bank_client = BankClient::new_shared(&bank);
        let ix = system_instruction::create_account(&alice_pubkey, &bob_pubkey, 1, len2, &program);
        let message = Message::new(&[ix], Some(&alice_pubkey));
        let r = bank_client.send_and_confirm_message(&[&alice_keypair, &bob_keypair], message);
        assert!(r.is_ok());
    }

    #[test]
    fn test_create_zero_lamport_with_clean() {
        with_create_zero_lamport(|bank| {
            bank.freeze();
            bank.squash();
            bank.force_flush_accounts_cache();
            // do clean and assert that it actually did its job
            assert_eq!(3, bank.get_snapshot_storages().len());
            bank.clean_accounts(false, false);
            assert_eq!(2, bank.get_snapshot_storages().len());
        });
    }

    #[test]
    fn test_create_zero_lamport_without_clean() {
        with_create_zero_lamport(|_| {
            // just do nothing; this should behave identically with test_create_zero_lamport_with_clean
        });
    }

    #[test]
    fn test_assign_with_seed() {
        let (genesis_config, mint_keypair) = create_genesis_config(100);
        let bank = Bank::new(&genesis_config);
        let bank_client = BankClient::new(bank);

        let alice_keypair = Keypair::new();
        let alice_pubkey = alice_keypair.pubkey();
        let seed = "seed";
        let owner = solana_sdk::pubkey::new_rand();
        let alice_with_seed = Pubkey::create_with_seed(&alice_pubkey, seed, &owner).unwrap();

        bank_client
            .transfer_and_confirm(50, &mint_keypair, &alice_pubkey)
            .unwrap();

        let assign_with_seed = Message::new(
            &[system_instruction::assign_with_seed(
                &alice_with_seed,
                &alice_pubkey,
                seed,
                &owner,
            )],
            Some(&alice_pubkey),
        );

        assert!(bank_client
            .send_and_confirm_message(&[&alice_keypair], assign_with_seed)
            .is_ok());
    }

    #[test]
    fn test_system_unsigned_transaction() {
        let (genesis_config, alice_keypair) = create_genesis_config(100);
        let alice_pubkey = alice_keypair.pubkey();
        let mallory_keypair = Keypair::new();
        let mallory_pubkey = mallory_keypair.pubkey();

        // Fund to account to bypass AccountNotFound error
        let bank = Bank::new(&genesis_config);
        let bank_client = BankClient::new(bank);
        bank_client
            .transfer_and_confirm(50, &alice_keypair, &mallory_pubkey)
            .unwrap();

        // Erroneously sign transaction with recipient account key
        // No signature case is tested by bank `test_zero_signatures()`
        let account_metas = vec![
            AccountMeta::new(alice_pubkey, false),
            AccountMeta::new(mallory_pubkey, true),
        ];
        let malicious_instruction = Instruction::new_with_bincode(
            system_program::id(),
            &SystemInstruction::Transfer { lamports: 10 },
            account_metas,
        );
        assert_eq!(
            bank_client
                .send_and_confirm_instruction(&mallory_keypair, malicious_instruction)
                .unwrap_err()
                .unwrap(),
            TransactionError::InstructionError(0, InstructionError::MissingRequiredSignature)
        );
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 50);
        assert_eq!(bank_client.get_balance(&mallory_pubkey).unwrap(), 50);
    }

    fn process_nonce_instruction(instruction: &Instruction) -> Result<(), InstructionError> {
        let accounts: Vec<_> = instruction
            .accounts
            .iter()
            .map(|meta| {
                RefCell::new(if sysvar::recent_blockhashes::check_id(&meta.pubkey) {
                    create_default_recent_blockhashes_account().into_inner()
                } else if sysvar::rent::check_id(&meta.pubkey) {
                    account::create_account_shared_data_for_test(&Rent::free())
                } else {
                    AccountSharedData::default()
                })
            })
            .collect();

        {
            let keyed_accounts: Vec<_> = instruction
                .accounts
                .iter()
                .zip(accounts.iter())
                .map(|(meta, account)| KeyedAccount::new(&meta.pubkey, meta.is_signer, account))
                .collect();
            process_instruction(&Pubkey::default(), keyed_accounts, &instruction.data)
        }
    }

    #[test]
    fn test_process_nonce_ix_no_acc_data_fail() {
        assert_eq!(
            process_nonce_instruction(&system_instruction::advance_nonce_account(
                &Pubkey::default(),
                &Pubkey::default()
            )),
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_process_nonce_ix_no_keyed_accs_fail() {
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![],
                &serialize(&SystemInstruction::AdvanceNonceAccount).unwrap()
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );
    }

    #[test]
    fn test_process_nonce_ix_only_nonce_acc_fail() {
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![KeyedAccount::new(
                    &Pubkey::default(),
                    true,
                    &create_default_account(),
                )],
                &serialize(&SystemInstruction::AdvanceNonceAccount).unwrap(),
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );
    }

    #[test]
    fn test_process_nonce_ix_bad_recent_blockhash_state_fail() {
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![
                    KeyedAccount::new(&Pubkey::default(), true, &create_default_account()),
                    KeyedAccount::new(
                        &sysvar::recent_blockhashes::id(),
                        false,
                        &create_default_account(),
                    ),
                ],
                &serialize(&SystemInstruction::AdvanceNonceAccount).unwrap(),
            ),
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_process_nonce_ix_ok() {
        let nonce_acc = nonce_account::create_account(1_000_000);
        process_instruction(
            &Pubkey::default(),
            vec![
                KeyedAccount::new(&Pubkey::default(), true, &nonce_acc),
                KeyedAccount::new(
                    &sysvar::recent_blockhashes::id(),
                    false,
                    &create_default_recent_blockhashes_account(),
                ),
                KeyedAccount::new(&sysvar::rent::id(), false, &create_default_rent_account()),
            ],
            &serialize(&SystemInstruction::InitializeNonceAccount(Pubkey::default())).unwrap(),
        )
        .unwrap();
        let new_recent_blockhashes_account = RefCell::new(
            solana_sdk::recent_blockhashes_account::create_account_with_data_for_test(
                vec![
                    IterItem(
                        0u64,
                        &hash(&serialize(&0).unwrap()),
                        &FeeCalculator::default()
                    );
                    sysvar::recent_blockhashes::MAX_ENTRIES
                ]
                .into_iter(),
            ),
        );
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![
                    KeyedAccount::new(&Pubkey::default(), true, &nonce_acc,),
                    KeyedAccount::new(
                        &sysvar::recent_blockhashes::id(),
                        false,
                        &new_recent_blockhashes_account,
                    ),
                ],
                &serialize(&SystemInstruction::AdvanceNonceAccount).unwrap(),
            ),
            Ok(()),
        );
    }

    #[test]
    fn test_process_withdraw_ix_no_acc_data_fail() {
        assert_eq!(
            process_nonce_instruction(&system_instruction::withdraw_nonce_account(
                &Pubkey::default(),
                &Pubkey::default(),
                &Pubkey::default(),
                1,
            )),
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_process_withdraw_ix_no_keyed_accs_fail() {
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![],
                &serialize(&SystemInstruction::WithdrawNonceAccount(42)).unwrap(),
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );
    }

    #[test]
    fn test_process_withdraw_ix_only_nonce_acc_fail() {
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![KeyedAccount::new(
                    &Pubkey::default(),
                    true,
                    &create_default_account()
                )],
                &serialize(&SystemInstruction::WithdrawNonceAccount(42)).unwrap(),
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );
    }

    #[test]
    fn test_process_withdraw_ix_bad_recent_blockhash_state_fail() {
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![
                    KeyedAccount::new(&Pubkey::default(), true, &create_default_account()),
                    KeyedAccount::new(&Pubkey::default(), false, &create_default_account()),
                    KeyedAccount::new(
                        &sysvar::recent_blockhashes::id(),
                        false,
                        &create_default_account()
                    ),
                ],
                &serialize(&SystemInstruction::WithdrawNonceAccount(42)).unwrap(),
            ),
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_process_withdraw_ix_bad_rent_state_fail() {
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![
                    KeyedAccount::new(
                        &Pubkey::default(),
                        true,
                        &nonce_account::create_account(1_000_000),
                    ),
                    KeyedAccount::new(&Pubkey::default(), true, &create_default_account()),
                    KeyedAccount::new(
                        &sysvar::recent_blockhashes::id(),
                        false,
                        &create_default_recent_blockhashes_account(),
                    ),
                    KeyedAccount::new(&sysvar::rent::id(), false, &create_default_account()),
                ],
                &serialize(&SystemInstruction::WithdrawNonceAccount(42)).unwrap(),
            ),
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_process_withdraw_ix_ok() {
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![
                    KeyedAccount::new(
                        &Pubkey::default(),
                        true,
                        &nonce_account::create_account(1_000_000),
                    ),
                    KeyedAccount::new(&Pubkey::default(), true, &create_default_account()),
                    KeyedAccount::new(
                        &sysvar::recent_blockhashes::id(),
                        false,
                        &create_default_recent_blockhashes_account(),
                    ),
                    KeyedAccount::new(&sysvar::rent::id(), false, &create_default_rent_account()),
                ],
                &serialize(&SystemInstruction::WithdrawNonceAccount(42)).unwrap(),
            ),
            Ok(()),
        );
    }

    #[test]
    fn test_process_initialize_ix_no_keyed_accs_fail() {
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![],
                &serialize(&SystemInstruction::InitializeNonceAccount(Pubkey::default())).unwrap(),
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );
    }

    #[test]
    fn test_process_initialize_ix_only_nonce_acc_fail() {
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![KeyedAccount::new(
                    &Pubkey::default(),
                    true,
                    &nonce_account::create_account(1_000_000),
                )],
                &serialize(&SystemInstruction::InitializeNonceAccount(Pubkey::default())).unwrap(),
            ),
            Err(InstructionError::NotEnoughAccountKeys),
        );
    }

    #[test]
    fn test_process_initialize_bad_recent_blockhash_state_fail() {
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![
                    KeyedAccount::new(
                        &Pubkey::default(),
                        true,
                        &nonce_account::create_account(1_000_000),
                    ),
                    KeyedAccount::new(
                        &sysvar::recent_blockhashes::id(),
                        false,
                        &create_default_account()
                    ),
                ],
                &serialize(&SystemInstruction::InitializeNonceAccount(Pubkey::default())).unwrap(),
            ),
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_process_initialize_ix_bad_rent_state_fail() {
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![
                    KeyedAccount::new(
                        &Pubkey::default(),
                        true,
                        &nonce_account::create_account(1_000_000),
                    ),
                    KeyedAccount::new(
                        &sysvar::recent_blockhashes::id(),
                        false,
                        &create_default_recent_blockhashes_account(),
                    ),
                    KeyedAccount::new(&sysvar::rent::id(), false, &create_default_account()),
                ],
                &serialize(&SystemInstruction::InitializeNonceAccount(Pubkey::default())).unwrap(),
            ),
            Err(InstructionError::InvalidArgument),
        );
    }

    #[test]
    fn test_process_initialize_ix_ok() {
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![
                    KeyedAccount::new(
                        &Pubkey::default(),
                        true,
                        &nonce_account::create_account(1_000_000),
                    ),
                    KeyedAccount::new(
                        &sysvar::recent_blockhashes::id(),
                        false,
                        &create_default_recent_blockhashes_account(),
                    ),
                    KeyedAccount::new(&sysvar::rent::id(), false, &create_default_rent_account()),
                ],
                &serialize(&SystemInstruction::InitializeNonceAccount(Pubkey::default())).unwrap(),
            ),
            Ok(()),
        );
    }

    #[test]
    fn test_process_authorize_ix_ok() {
        let nonce_acc = nonce_account::create_account(1_000_000);
        process_instruction(
            &Pubkey::default(),
            vec![
                KeyedAccount::new(&Pubkey::default(), true, &nonce_acc),
                KeyedAccount::new(
                    &sysvar::recent_blockhashes::id(),
                    false,
                    &create_default_recent_blockhashes_account(),
                ),
                KeyedAccount::new(&sysvar::rent::id(), false, &create_default_rent_account()),
            ],
            &serialize(&SystemInstruction::InitializeNonceAccount(Pubkey::default())).unwrap(),
        )
        .unwrap();
        assert_eq!(
            process_instruction(
                &Pubkey::default(),
                vec![KeyedAccount::new(&Pubkey::default(), true, &nonce_acc,),],
                &serialize(&SystemInstruction::AuthorizeNonceAccount(Pubkey::default(),)).unwrap(),
            ),
            Ok(()),
        );
    }

    #[test]
    fn test_process_authorize_bad_account_data_fail() {
        assert_eq!(
            process_nonce_instruction(&system_instruction::authorize_nonce_account(
                &Pubkey::default(),
                &Pubkey::default(),
                &Pubkey::default(),
            )),
            Err(InstructionError::InvalidAccountData),
        );
    }

    #[test]
    fn test_get_system_account_kind_system_ok() {
        let system_account = AccountSharedData::default();
        assert_eq!(
            get_system_account_kind(&system_account),
            Some(SystemAccountKind::System)
        );
    }

    #[test]
    fn test_get_system_account_kind_nonce_ok() {
        let nonce_account = AccountSharedData::new_data(
            42,
            &nonce::state::Versions::new_current(nonce::State::Initialized(
                nonce::state::Data::default(),
            )),
            &system_program::id(),
        )
        .unwrap();
        assert_eq!(
            get_system_account_kind(&nonce_account),
            Some(SystemAccountKind::Nonce)
        );
    }

    #[test]
    fn test_get_system_account_kind_uninitialized_nonce_account_fail() {
        assert_eq!(
            get_system_account_kind(&nonce_account::create_account(42).borrow()),
            None
        );
    }

    #[test]
    fn test_get_system_account_kind_system_owner_nonzero_nonnonce_data_fail() {
        let other_data_account =
            AccountSharedData::new_data(42, b"other", &Pubkey::default()).unwrap();
        assert_eq!(get_system_account_kind(&other_data_account), None);
    }

    #[test]
    fn test_get_system_account_kind_nonsystem_owner_with_nonce_data_fail() {
        let nonce_account = AccountSharedData::new_data(
            42,
            &nonce::state::Versions::new_current(nonce::State::Initialized(
                nonce::state::Data::default(),
            )),
            &solana_sdk::pubkey::new_rand(),
        )
        .unwrap();
        assert_eq!(get_system_account_kind(&nonce_account), None);
    }
}
